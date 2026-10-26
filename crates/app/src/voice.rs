//! Voice in: microphone (cpal, hold to talk) -> core::meta::pcm_f32_to_wav16k -> transcribe.
//! Voice out: `Speaker` trait; `tts` crate (WinRT/SAPI on Windows, speech-dispatcher on Linux);
//! Windows fallback: PowerShell System.Speech. `PIP_MUTE=1` silences Pip.

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
        crate::backend::guard("pcm_f32_to_wav16k", || {
            pip_core::meta::pcm_f32_to_wav16k(&c.samples, c.rate, c.channels)
        })
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
                if let Some(t) = tts.as_mut() {
                    let _ = t.stop();
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
