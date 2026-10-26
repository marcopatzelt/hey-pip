//! Speech to text: POST {base}/asr/transcribe, multipart/form-data:
//! part `request` (application/json) = {"model":"muse-voice-transcribe-1.0","audioEncoding":"WAV",
//! "mode":"PUSH_TO_TALK","languageBias":["German"]}, part `audio` (audio/wav), header Accept: application/json.
//! WAV: mono 16-bit PCM, 16 or 24 kHz, max 10 min / 32 MB. Response {sessionId, transcript, audioDurationMs, turns}.
//! OWNER: agent "meta".

use anyhow::{Result, bail};
use reqwest::multipart::{Form, Part};
use serde_json::{Value, json};

use super::client::{MetaClient, MetaError};
use crate::i18n::Lang;

/// API limit for one upload.
pub const MAX_WAV_BYTES: usize = 32 * 1024 * 1024;

fn request_json(lang: Lang) -> Value {
    let bias = match lang {
        Lang::De => "German",
        Lang::En => "English",
    };
    json!({
        "model": super::MODEL_ASR,
        "audioEncoding": "WAV",
        "mode": "PUSH_TO_TALK",
        "languageBias": [bias],
    })
}

impl MetaClient {
    /// Returns `transcript` (trimmed; empty if nothing was said).
    /// languageBias from `lang` (De -> "German", En -> "English").
    pub async fn transcribe(&self, wav: Vec<u8>, lang: Lang) -> Result<String> {
        if wav.len() > MAX_WAV_BYTES {
            bail!("recording too long ({} bytes, max {MAX_WAV_BYTES})", wav.len());
        }
        let request = request_json(lang).to_string();
        let url = self.url("asr/transcribe");
        let resp = self
            .send("asr/transcribe", || {
                let form = Form::new()
                    .part("request", Part::text(request.clone()).mime_str("application/json").expect("mime"))
                    .part(
                        "audio",
                        Part::bytes(wav.clone()).file_name("audio.wav").mime_str("audio/wav").expect("mime"),
                    );
                self.http.post(&url).header(reqwest::header::ACCEPT, "application/json").multipart(form)
            })
            .await?;
        let text = resp.text().await.map_err(|e| MetaError::Network(e.without_url()))?;
        let v: Value = serde_json::from_str(&text).map_err(|e| MetaError::Parse(format!("asr: {e}")))?;
        let transcript = v
            .get("transcript")
            .and_then(Value::as_str)
            .ok_or_else(|| MetaError::Parse("asr answer without transcript".into()))?;
        Ok(transcript.trim().to_string())
    }
}

/// Mono i16 samples -> in-memory WAV (hound).
pub fn wav_from_pcm16(samples: &[i16], sample_rate: u32) -> Result<Vec<u8>> {
    let spec =
        hound::WavSpec { channels: 1, sample_rate, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut cur = std::io::Cursor::new(Vec::with_capacity(44 + samples.len() * 2));
    {
        let mut w = hound::WavWriter::new(&mut cur, spec)?;
        let mut w16 = w.get_i16_writer(samples.len() as u32);
        for &s in samples {
            w16.write_sample(s);
        }
        w16.flush()?;
        w.finalize()?;
    }
    Ok(cur.into_inner())
}

/// Interleaved f32 samples from the mic (any rate/channels) -> mono 16 kHz 16-bit WAV.
/// Downmix by averaging, linear resample.
pub fn pcm_f32_to_wav16k(samples: &[f32], sample_rate: u32, channels: u16) -> Result<Vec<u8>> {
    if sample_rate == 0 || channels == 0 {
        bail!("invalid audio format: {sample_rate} Hz, {channels} channels");
    }
    let ch = usize::from(channels);
    let mono: Vec<f32> = samples.chunks_exact(ch).map(|f| f.iter().sum::<f32>() / ch as f32).collect();
    const TARGET: u32 = 16_000;
    let out: Vec<i16> = resample_linear(&mono, sample_rate, TARGET)
        .into_iter()
        .map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16)
        .collect();
    wav_from_pcm16(&out, TARGET)
}

fn resample_linear(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.len() < 2 {
        return input.to_vec();
    }
    let ratio = f64::from(from) / f64::from(to);
    let n = ((input.len() as f64) / ratio).floor() as usize;
    (0..n)
        .map(|i| {
            let pos = i as f64 * ratio;
            let j = pos.floor() as usize;
            let frac = (pos - j as f64) as f32;
            let a = input[j.min(input.len() - 1)];
            let b = input[(j + 1).min(input.len() - 1)];
            a + (b - a) * frac
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(wav: &[u8]) -> (hound::WavSpec, Vec<i16>) {
        let r = hound::WavReader::new(std::io::Cursor::new(wav)).unwrap();
        let spec = r.spec();
        (spec, r.into_samples::<i16>().map(Result::unwrap).collect())
    }

    #[test]
    fn pcm16_roundtrip() {
        let wav = wav_from_pcm16(&[0, 1000, -1000, i16::MAX], 16_000).unwrap();
        assert_eq!(&wav[..4], b"RIFF");
        let (spec, s) = read(&wav);
        assert_eq!((spec.channels, spec.sample_rate, spec.bits_per_sample), (1, 16_000, 16));
        assert_eq!(s, vec![0, 1000, -1000, i16::MAX]);
    }

    #[test]
    fn stereo_48k_to_mono_16k() {
        // 1 s of stereo 48 kHz: left 0.5, right -0.5 except a constant 0.25 on both.
        let frames = 48_000;
        let mut v = Vec::with_capacity(frames * 2);
        for _ in 0..frames {
            v.push(0.75);
            v.push(-0.25);
        }
        let (spec, s) = read(&pcm_f32_to_wav16k(&v, 48_000, 2).unwrap());
        assert_eq!((spec.channels, spec.sample_rate), (1, 16_000));
        assert_eq!(s.len(), 16_000);
        let expect = (0.25 * i16::MAX as f32).round() as i16;
        assert!(s.iter().all(|&x| x == expect));
    }

    #[test]
    fn clamps_and_rejects_bad_format() {
        let (_, s) = read(&pcm_f32_to_wav16k(&[2.0, -2.0], 16_000, 1).unwrap());
        assert_eq!(s, vec![i16::MAX, -i16::MAX]);
        assert!(pcm_f32_to_wav16k(&[0.0], 0, 1).is_err());
        assert!(pcm_f32_to_wav16k(&[0.0], 16_000, 0).is_err());
    }

    #[test]
    fn resample_44k1() {
        let input: Vec<f32> = (0..44_100).map(|i| i as f32 / 44_100.0).collect();
        let out = resample_linear(&input, 44_100, 16_000);
        assert_eq!(out.len(), 16_000);
        assert!((out[8000] - 0.5).abs() < 1e-3);
    }

    #[test]
    fn request_part() {
        let v = request_json(Lang::De);
        assert_eq!(v["model"], "muse-voice-transcribe-1.0");
        assert_eq!(v["languageBias"][0], "German");
        assert_eq!(request_json(Lang::En)["languageBias"][0], "English");
    }
}
