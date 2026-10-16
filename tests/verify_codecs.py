#!/usr/bin/env python3
"""Generate real encoded fixtures, run the app decoder, validate sample duration."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile

parser = argparse.ArgumentParser()
parser.add_argument("binary", type=Path)
parser.add_argument("--report", type=Path)
parser.add_argument("--extended", action="store_true", help="Verify the optional FFmpeg fallback too")
args = parser.parse_args()
binary = args.binary.resolve()
formats = [
    ("wav", "pcm_s16le"), ("flac", "flac"), ("mp3", "libmp3lame"),
    ("ogg", "libvorbis"), ("m4a", "aac"), ("aac", "aac"),
    ("alac.m4a", "alac"), ("aiff", "pcm_s16be"), ("mp2", "mp2"),
    ("opus", "libopus"),
]
if args.extended:
    formats += [("wma1.wma", "wmav1"), ("wma", "wmav2"), ("wv", "wavpack"), ("caf", "pcm_s16le")]
results = []
encoders = subprocess.run(["ffmpeg", "-hide_banner", "-encoders"], check=True,
                          capture_output=True, text=True).stdout
with tempfile.TemporaryDirectory(prefix="auvemir-codecs-") as directory:
    for extension, codec in formats:
        path = Path(directory) / ("tone." + extension)
        # Minimal FFmpeg distributions may supply the native Vorbis encoder
        # without libvorbis. This still produces a real Vorbis fixture; no
        # decoder checks are skipped. The native encoder requires stereo.
        native_vorbis = codec == "libvorbis" and not re.search(r"\blibvorbis\s", encoders)
        if native_vorbis:
            codec = "vorbis"
        command = ["ffmpeg", "-hide_banner", "-loglevel", "error", "-f", "lavfi",
                   "-i", "sine=frequency=440:duration=1", "-c:a", codec]
        if native_vorbis:
            command += ["-strict", "-2", "-ac", "2"]
        # 384 kbit/s mono MPEG Layer II is rejected by the strict native demuxer.
        if extension == "mp2":
            command += ["-b:a", "128k"]
        generated = subprocess.run(command + ["-y", str(path)], capture_output=True, text=True)
        if generated.returncode:
            results.append({"format": extension, "result": "fail", "stage": "fixture generation",
                            "fixture_encoder": codec, "output": generated.stderr.strip()[:1000]})
            continue
        result = subprocess.run([str(binary), "--decode", str(path)], capture_output=True,
                                text=True, timeout=20, env={**os.environ, "RUST_LOG": "error"})
        match = re.search(r"Decoded (\d+) samples.*?(\d+) channels.*?(\d+) Hz", result.stdout)
        duration = (int(match[1]) / (int(match[2]) * int(match[3]))) if match else 0
        ok = result.returncode == 0 and 0.9 <= duration <= 1.15
        results.append({"format": extension, "result": "pass" if ok else "fail",
                        "fixture_encoder": codec,
                        "decoded_seconds": round(duration, 6),
                        "output": (result.stdout + result.stderr).strip()[:1000]})
report = json.dumps(results, indent=2) + "\n"
print(report, end="")
if args.report:
    args.report.write_text(report)
raise SystemExit(0 if all(result["result"] == "pass" for result in results) else 1)
