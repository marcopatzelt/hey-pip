//! Voice in: microphone (cpal, hold to talk) -> core::meta::pcm_f32_to_wav16k -> transcribe.
//! Voice out: `Speaker` trait; `tts` crate (WinRT/SAPI on Windows, speech-dispatcher on Linux);
//! Windows fallback: PowerShell System.Speech. `PIP_MUTE=1` silences Pip.
//! Linux and Windows: English goes through Piper (open-source neural voice; `linux_piper`, `win_piper`) when it
//! is installed; else the system voice as before. `PIP_VOICE` picks the Piper voice, `PIP_PIPER=0` turns it off.

use std::sync::mpsc as std_mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use pip_core::i18n::Lang;

pub trait Speaker: Send + Sync {
    fn speak(&self, text: &str, lang: Lang) -> Result<()>;
    fn stop(&self) -> Result<()>;
}

/// Longest recording we keep (the API allows 10 min; a spoken command is short).
const MAX_SECONDS: usize = 120;
/// Shorter than this is a slip of the finger, not speech.
pub const MIN_SECONDS: f32 = 0.4;

/// The user only tapped the microphone button.
#[derive(Debug)]
pub struct TooShort;

impl std::fmt::Display for TooShort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("recording too short")
    }
}
impl std::error::Error for TooShort {}

struct Captured {
    samples: Vec<f32>,
    rate: u32,
    channels: u16,
}

/// Records while held. `start` opens the default input device; `stop` returns WAV bytes (mono 16 kHz).
/// The cpal stream lives on its own thread (it is not `Send` on every platform).
pub struct Recorder {
    stop_tx: std_mpsc::Sender<()>,
    join: JoinHandle<Result<Captured>>,
}

impl Recorder {
    pub fn start() -> Result<Self> {
        let (stop_tx, stop_rx) = std_mpsc::channel::<()>();
        let (ready_tx, ready_rx) = std_mpsc::channel::<Result<()>>();
        let join = std::thread::Builder::new().name("pip-mic".into()).spawn(move || {
            let r = record(stop_rx, &ready_tx);
            if let Err(e) = &r {
                let _ = ready_tx.send(Err(anyhow!("{e:#}")));
            }
            r
        })?;
        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(Self { stop_tx, join }),
            Ok(Err(e)) => Err(e),
            Err(_) => bail!("microphone did not start"),
        }
    }

    /// WAV bytes, mono 16 kHz 16-bit. `TooShort` if the user only tapped.
    pub fn stop(self) -> Result<Vec<u8>> {
        let _ = self.stop_tx.send(());
        let c = self.join.join().map_err(|_| anyhow!("microphone thread panicked"))??;
        let secs = c.samples.len() as f32 / (c.rate as f32 * c.channels.max(1) as f32);
        if secs < MIN_SECONDS {
            return Err(TooShort.into());
        }
        crate::backend::guard("pcm_f32_to_wav16k", || pip_core::meta::pcm_f32_to_wav16k(&c.samples, c.rate, c.channels))
    }
}

fn record(stop_rx: std_mpsc::Receiver<()>, ready: &std_mpsc::Sender<Result<()>>) -> Result<Captured> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    let host = cpal::default_host();
    let device = host.default_input_device().context("no microphone")?;
    let config = device.default_input_config().context("microphone config")?;
    let rate = config.sample_rate().0;
    let channels = config.channels();
    let cap = MAX_SECONDS * rate as usize * channels as usize;
    let buf = Arc::new(Mutex::new(Vec::<f32>::with_capacity(rate as usize * channels as usize * 8)));
    let err_fn = |e| tracing::warn!("microphone stream: {e}");
    let stream = {
        let b = buf.clone();
        let push = move |it: &mut dyn Iterator<Item = f32>| {
            let mut v = b.lock().expect("mic buffer");
            let room = cap.saturating_sub(v.len());
            v.extend(it.take(room));
        };
        let cfg = config.config();
        match config.sample_format() {
            cpal::SampleFormat::F32 => {
                device.build_input_stream(&cfg, move |d: &[f32], _: &_| push(&mut d.iter().copied()), err_fn, None)?
            }
            cpal::SampleFormat::I16 => device.build_input_stream(
                &cfg,
                move |d: &[i16], _: &_| push(&mut d.iter().map(|s| *s as f32 / i16::MAX as f32)),
                err_fn,
                None,
            )?,
            cpal::SampleFormat::U16 => device.build_input_stream(
                &cfg,
                move |d: &[u16], _: &_| push(&mut d.iter().map(|s| (*s as f32 - 32768.0) / 32768.0)),
                err_fn,
                None,
            )?,
            other => bail!("unsupported microphone format {other:?}"),
        }
    };
    stream.play()?;
    let _ = ready.send(Ok(()));
    // Wait for stop (or the Recorder being dropped).
    let _ = stop_rx.recv_timeout(Duration::from_secs(MAX_SECONDS as u64 + 5));
    drop(stream);
    let samples = std::mem::take(&mut *buf.lock().expect("mic buffer"));
    Ok(Captured { samples, rate, channels })
}

/// Speaks on its own thread, one sentence after the other; a new text interrupts the old one.
pub struct TtsSpeaker {
    tx: std_mpsc::Sender<SpeakMsg>,
}

enum SpeakMsg {
    Say(String, Lang),
    Stop,
}

impl TtsSpeaker {
    pub fn new() -> Self {
        let (tx, rx) = std_mpsc::channel::<SpeakMsg>();
        let _ = std::thread::Builder::new().name("pip-voice".into()).spawn(move || speaker_thread(rx));
        Self { tx }
    }
}

impl Speaker for TtsSpeaker {
    fn speak(&self, text: &str, lang: Lang) -> Result<()> {
        self.tx.send(SpeakMsg::Say(text.to_string(), lang)).map_err(|_| anyhow!("voice thread gone"))
    }
    fn stop(&self) -> Result<()> {
        self.tx.send(SpeakMsg::Stop).map_err(|_| anyhow!("voice thread gone"))
    }
}

fn speaker_thread(rx: std_mpsc::Receiver<SpeakMsg>) {
    #[cfg(any(target_os = "linux", windows))]
    let mut piper = piper_voice::Piper::start();
    let mut tts = match tts::Tts::default() {
        Ok(t) => Some(t),
        Err(e) => {
            tracing::warn!("text to speech not available ({e}); using fallback");
            None
        }
    };
    if let Some(t) = tts.as_mut() {
        // A little slower than normal: calm, easy to follow.
        let rate = t.normal_rate() - (t.normal_rate() - t.min_rate()) * 0.15;
        let _ = t.set_rate(rate);
    }
    let mut voice_lang: Option<Lang> = None;
    while let Ok(msg) = rx.recv() {
        match msg {
            SpeakMsg::Stop => {
                #[cfg(any(target_os = "linux", windows))]
                if let Some(p) = piper.as_mut() {
                    p.stop();
                }
                if let Some(t) = tts.as_mut() {
                    let _ = t.stop();
                }
            }
            #[cfg(any(target_os = "linux", windows))]
            SpeakMsg::Say(text, lang) if lang == Lang::En && piper.is_some() => {
                let p = piper.as_mut().expect("piper");
                if let Err(e) = p.say(&text) {
                    tracing::warn!("piper voice failed ({e:#}); back to the system voice");
                    p.stop();
                    piper = None;
                    if let Some(t) = tts.as_mut() {
                        pick_voice(t, lang);
                        let _ = t.speak(text, true);
                    }
                }
            }
            SpeakMsg::Say(text, lang) => match tts.as_mut() {
                Some(t) => {
                    if voice_lang != Some(lang) {
                        pick_voice(t, lang);
                        voice_lang = Some(lang);
                    }
                    if let Err(e) = t.speak(text, true) {
                        tracing::warn!("speak failed: {e}");
                    }
                }
                None => fallback_speak(&text, lang),
            },
        }
    }
}

fn pick_voice(t: &mut tts::Tts, lang: Lang) {
    let Ok(voices) = t.voices() else { return };
    let want = lang.code();
    if let Some(v) = voices.iter().find(|v| v.language().as_str().to_ascii_lowercase().starts_with(want)) {
        let _ = t.set_voice(v);
    }
}

#[cfg(windows)]
fn fallback_speak(text: &str, lang: Lang) {
    use std::io::Write;
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let culture = if lang == Lang::En { "en-US" } else { "de-DE" };
    // The text goes in over stdin, never into the command line.
    let script = format!(
        "Add-Type -AssemblyName System.Speech; $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; \
         try {{ $s.SelectVoiceByHints('NotSet','NotSet',0,[Globalization.CultureInfo]'{culture}') }} catch {{}}; \
         $s.Rate = -1; $s.Speak([Console]::In.ReadToEnd())"
    );
    let child = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .stdin(std::process::Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
    match child {
        Ok(mut c) => {
            if let Some(mut stdin) = c.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = c.wait();
        }
        Err(e) => tracing::warn!("powershell speech failed: {e}"),
    }
}

#[cfg(not(windows))]
fn fallback_speak(_text: &str, _lang: Lang) {}

/// Linux voice: Piper, an open-source neural text-to-speech model, in a long-lived Python process
/// (`crates/app/tools/piper_speak.py`, venv and voices in the git-ignored `tools/piper/`). The text goes in over
/// stdin as JSON, never on a command line; the WAV is played with `pw-play` (or `paplay`) and cut off when the
/// next sentence or a stop comes.
#[cfg(target_os = "linux")]
mod linux_piper {
    use std::io::{BufRead, BufReader, Write};
    use std::path::{Path, PathBuf};
    use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

    use anyhow::{Context, Result, bail};

    /// Warm, clear US English voice; others in tools/piper/voices via PIP_VOICE (e.g. en_GB-jenny_dioco-medium).
    const DEFAULT_VOICE: &str = "en_US-lessac-medium";

    pub struct Piper {
        child: Child,
        stdin: ChildStdin,
        stdout: BufReader<ChildStdout>,
        player: Option<Child>,
        player_cmd: &'static str,
        n: u64,
    }

    /// The repo root: from `PIP_PIPER_DIR`'s parent, the current folder, or the binary's place (target/release).
    fn find_root() -> Option<PathBuf> {
        let mut starts: Vec<PathBuf> = Vec::new();
        if let Ok(d) = std::env::current_dir() {
            starts.push(d);
        }
        if let Ok(exe) = std::env::current_exe() {
            starts.push(exe);
        }
        starts.into_iter().find_map(|s| {
            s.ancestors().find(|a| a.join("tools/piper/.venv/bin/python").is_file()).map(Path::to_path_buf)
        })
    }

    fn which(cmd: &str) -> bool {
        std::env::var_os("PATH")
            .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(cmd).is_file()))
    }

    impl Piper {
        /// None if Piper is turned off (`PIP_PIPER=0`) or not installed: then the system voice speaks.
        pub fn start() -> Option<Self> {
            if std::env::var("PIP_PIPER").is_ok_and(|v| v == "0") {
                return None;
            }
            match Self::try_start() {
                Ok(p) => Some(p),
                Err(e) => {
                    tracing::info!("piper voice not used: {e:#}");
                    None
                }
            }
        }

        fn try_start() -> Result<Self> {
            let (piper_dir, script) = match std::env::var_os("PIP_PIPER_DIR") {
                Some(d) => {
                    let d = PathBuf::from(d);
                    let root = d.parent().and_then(Path::parent).map(Path::to_path_buf).unwrap_or_default();
                    (d, root.join("crates/app/tools/piper_speak.py"))
                }
                None => {
                    let root = find_root().context("tools/piper is not installed")?;
                    (root.join("tools/piper"), root.join("crates/app/tools/piper_speak.py"))
                }
            };
            let voice = std::env::var("PIP_VOICE").unwrap_or_else(|_| DEFAULT_VOICE.to_string());
            if voice.contains('/') || voice.contains("..") {
                bail!("bad PIP_VOICE");
            }
            let model = piper_dir.join("voices").join(format!("{voice}.onnx"));
            if !model.is_file() {
                bail!("voice {voice} missing in {}", piper_dir.display());
            }
            if !script.is_file() {
                bail!("{} missing", script.display());
            }
            let player_cmd = if which("pw-play") {
                "pw-play"
            } else if which("paplay") {
                "paplay"
            } else {
                bail!("no pw-play or paplay")
            };
            // A little slower than normal: calm, easy to follow (same idea as the system voice's rate).
            let length = std::env::var("PIP_VOICE_PACE").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.08);
            let mut child = Command::new(piper_dir.join(".venv/bin/python"))
                .arg("-I")
                .arg(&script)
                .arg(&model)
                .arg(format!("{length:.2}"))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .context("start piper")?;
            let stdin = child.stdin.take().context("piper stdin")?;
            let mut stdout = BufReader::new(child.stdout.take().context("piper stdout")?);
            let mut line = String::new();
            stdout.read_line(&mut line).context("piper did not start")?;
            if line.trim() != "ready" {
                let _ = child.kill();
                let _ = child.wait();
                bail!("piper did not start");
            }
            tracing::info!(voice, player = player_cmd, "voice: piper");
            Ok(Self { child, stdin, stdout, player: None, player_cmd, n: 0 })
        }

        /// Cut off what is playing.
        pub fn stop(&mut self) {
            if let Some(mut p) = self.player.take() {
                if matches!(p.try_wait(), Ok(None)) {
                    tracing::debug!("piper: cut off");
                }
                let _ = p.kill();
                let _ = p.wait();
            }
        }

        /// Synthesize, then play without waiting (the next sentence or a stop cuts it off).
        pub fn say(&mut self, text: &str) -> Result<()> {
            self.stop();
            let text: String = text.chars().filter(|c| !c.is_control() || *c == ' ').collect();
            if text.trim().is_empty() {
                return Ok(());
            }
            // Two files in turn: the one playing is never overwritten by the next.
            self.n += 1;
            let out = std::env::temp_dir().join(format!("pip-voice-{}-{}.wav", std::process::id(), self.n % 2));
            let msg = serde_json::json!({ "text": text, "out": out.to_string_lossy() });
            writeln!(self.stdin, "{msg}").context("write to piper")?;
            self.stdin.flush().context("write to piper")?;
            let mut line = String::new();
            if self.stdout.read_line(&mut line).context("read from piper")? == 0 {
                bail!("piper ended");
            }
            if line.trim() != "ok" {
                bail!("piper: {}", line.trim());
            }
            let player = Command::new(self.player_cmd)
                .arg(&out)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .context("start the player")?;
            tracing::debug!(chars = text.chars().count(), "piper: speaking");
            self.player = Some(player);
            Ok(())
        }
    }

    impl Drop for Piper {
        fn drop(&mut self) {
            self.stop();
            let _ = self.child.kill();
            let _ = self.child.wait();
            for i in 0..2 {
                let _ = std::fs::remove_file(
                    std::env::temp_dir().join(format!("pip-voice-{}-{i}.wav", std::process::id())),
                );
            }
        }
    }
}

#[cfg(target_os = "linux")]
use linux_piper as piper_voice;
#[cfg(windows)]
use win_piper as piper_voice;

/// Windows voice: Piper's own program (`piper\piper.exe` next to Pip.exe, voices in `piper\voices`), started once
/// with `--json-input`: one JSON line per sentence on stdin (`text`, `output_file`), it answers with the WAV path.
/// The text never goes on a command line. The WAV plays with PlaySound (async), cut off by the next sentence or a
/// stop. No piper folder: the Windows voice speaks as before.
#[cfg(windows)]
mod win_piper {
    use std::io::{BufRead, BufReader, Write};
    use std::os::windows::process::CommandExt;
    use std::path::PathBuf;
    use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

    use anyhow::{Context, Result, bail};
    use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME, SND_NODEFAULT};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const DEFAULT_VOICE: &str = "en_US-lessac-medium";

    pub struct Piper {
        child: Child,
        stdin: ChildStdin,
        stdout: BufReader<ChildStdout>,
        n: u64,
    }

    fn piper_dir() -> Option<PathBuf> {
        if let Some(d) = std::env::var_os("PIP_PIPER_DIR") {
            return Some(PathBuf::from(d));
        }
        let exe = std::env::current_exe().ok()?;
        Some(exe.parent()?.join("piper"))
    }

    fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        s.encode_wide().chain(std::iter::once(0)).collect()
    }

    impl Piper {
        pub fn start() -> Option<Self> {
            if std::env::var("PIP_PIPER").is_ok_and(|v| v == "0") {
                return None;
            }
            match Self::try_start() {
                Ok(p) => Some(p),
                Err(e) => {
                    tracing::info!("piper voice not used: {e:#}");
                    None
                }
            }
        }

        fn try_start() -> Result<Self> {
            let dir = piper_dir().context("no piper folder")?;
            let exe = dir.join("piper.exe");
            if !exe.is_file() {
                bail!("{} missing", exe.display());
            }
            let voice = std::env::var("PIP_VOICE").unwrap_or_else(|_| DEFAULT_VOICE.to_string());
            if voice.contains(['/', '\\']) || voice.contains("..") {
                bail!("bad PIP_VOICE");
            }
            let model = dir.join("voices").join(format!("{voice}.onnx"));
            if !model.is_file() {
                bail!("voice {voice} missing in {}", dir.display());
            }
            let length = std::env::var("PIP_VOICE_PACE").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.08);
            let mut child = Command::new(&exe)
                .arg("--model")
                .arg(&model)
                .arg("--json-input")
                .arg("--length_scale")
                .arg(format!("{length:.2}"))
                .arg("--output_dir")
                .arg(std::env::temp_dir())
                .current_dir(&dir)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()
                .context("start piper")?;
            std::thread::sleep(std::time::Duration::from_millis(300));
            if let Ok(Some(st)) = child.try_wait() {
                bail!("piper ended at once ({st})");
            }
            let stdin = child.stdin.take().context("piper stdin")?;
            let stdout = BufReader::new(child.stdout.take().context("piper stdout")?);
            tracing::info!(voice, "voice: piper");
            Ok(Self { child, stdin, stdout, n: 0 })
        }

        pub fn stop(&mut self) {
            // A null sound stops whatever PlaySound plays.
            unsafe { PlaySoundW(std::ptr::null(), std::ptr::null_mut(), 0) };
        }

        pub fn say(&mut self, text: &str) -> Result<()> {
            self.stop();
            let text: String = text.chars().filter(|c| !c.is_control() || *c == ' ').collect();
            if text.trim().is_empty() {
                return Ok(());
            }
            self.n += 1;
            let out = std::env::temp_dir().join(format!("pip-voice-{}-{}.wav", std::process::id(), self.n % 2));
            let msg = serde_json::json!({ "text": text, "output_file": out.to_string_lossy() });
            writeln!(self.stdin, "{msg}").context("write to piper")?;
            self.stdin.flush().context("write to piper")?;
            let mut line = String::new();
            if self.stdout.read_line(&mut line).context("read from piper")? == 0 {
                bail!("piper ended");
            }
            // piper prints the path just before it closes the file.
            std::thread::sleep(std::time::Duration::from_millis(60));
            let path = wide(out.as_os_str());
            let ok = unsafe { PlaySoundW(path.as_ptr(), std::ptr::null_mut(), SND_FILENAME | SND_ASYNC | SND_NODEFAULT) };
            if ok == 0 {
                bail!("PlaySound failed");
            }
            tracing::debug!(chars = text.chars().count(), "piper: speaking");
            Ok(())
        }
    }

    impl Drop for Piper {
        fn drop(&mut self) {
            self.stop();
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// Used when PIP_MUTE=1.
pub struct Silent;

impl Speaker for Silent {
    fn speak(&self, _text: &str, _lang: Lang) -> Result<()> {
        Ok(())
    }
    fn stop(&self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn too_short_is_detectable() {
        let e: anyhow::Error = TooShort.into();
        assert!(e.downcast_ref::<TooShort>().is_some());
    }

    /// Opens the real microphone for one second (audio stays in memory). `cargo test -p pip-app -- --ignored`
    #[test]
    #[ignore]
    fn records_one_second_as_wav16k() {
        let rec = Recorder::start().expect("microphone");
        std::thread::sleep(Duration::from_millis(1000));
        let wav = rec.stop().expect("wav");
        let r = hound::WavReader::new(std::io::Cursor::new(&wav)).expect("valid wav");
        let spec = r.spec();
        assert_eq!((spec.channels, spec.sample_rate, spec.bits_per_sample), (1, 16_000, 16));
        assert!(r.duration() > 12_000, "about one second of audio, got {} samples", r.duration());
    }

    #[test]
    #[ignore]
    fn tap_is_too_short() {
        let Ok(rec) = Recorder::start() else { return }; // no microphone
        let e = rec.stop().unwrap_err();
        assert!(e.downcast_ref::<TooShort>().is_some(), "{e:#}");
    }

    #[test]
    fn silent_speaker_is_ok() {
        assert!(Silent.speak("Hallo", Lang::De).is_ok());
        assert!(Silent.stop().is_ok());
    }
}
