#!/usr/bin/env python3
"""Pip's Linux voice: a long-lived Piper (open-source neural TTS) process.

Pip (crates/app/src/voice.rs, PiperSpeaker) starts this once and sends one JSON line per sentence on stdin:
    {"text": "...", "out": "/path/to/file.wav"}
The script writes the WAV and answers with one line on stdout: "ok" or "err <reason>".
Pip plays the file itself (pw-play / paplay), so it can cut a sentence off when a new one comes.
The text never goes on a command line. Only the model path does.

Usage: piper_speak.py MODEL.onnx [LENGTH_SCALE]
"""
import json
import sys
import wave


def main() -> int:
    if len(sys.argv) < 2:
        print("usage: piper_speak.py MODEL.onnx [LENGTH_SCALE]", file=sys.stderr)
        return 2
    from piper import PiperVoice, SynthesisConfig

    voice = PiperVoice.load(sys.argv[1])
    length = float(sys.argv[2]) if len(sys.argv) > 2 else 1.0
    cfg = SynthesisConfig(length_scale=length)
    print("ready", flush=True)
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
            with wave.open(msg["out"], "wb") as w:
                voice.synthesize_wav(str(msg["text"]), w, syn_config=cfg)
            print("ok", flush=True)
        except Exception as e:  # noqa: BLE001 - one bad sentence must not end the voice
            print(f"err {type(e).__name__}", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
