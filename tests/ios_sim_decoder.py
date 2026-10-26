#!/usr/bin/env python3
"""Capture decoder output in files before launching the simulator app."""
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time

DECODE_TIMEOUT = 15

def main(arguments=None):
    arguments = sys.argv[1:] if arguments is None else arguments
    if len(arguments) != 2 or arguments[0] != "--decode":
        raise SystemExit("Usage: ios_sim_decoder.py --decode AUDIO")
    device = os.environ["AUVEMIR_SIMULATOR_ID"]
    source = Path(arguments[1])
    destination = Path(os.environ["AUVEMIR_SIMULATOR_DATA"]) / "Documents" / source.name
    destination.parent.mkdir(exist_ok=True)
    shutil.copyfile(source, destination)
    with tempfile.TemporaryDirectory(prefix="auvemir-sim-output-") as directory:
        output = Path(directory) / "stdout.txt"
        errors = Path(directory) / "stderr.txt"
        output.touch()
        errors.touch()
        deadline = time.monotonic() + DECODE_TIMEOUT
        launch = subprocess.run(
            ["xcrun", "simctl", "launch", f"--stdout={output}", f"--stderr={errors}",
             "--terminate-running-process", device, "org.auvemir.app", "--decode", str(destination)],
            capture_output=True, text=True, timeout=10,
        )
        try:
            while launch.returncode == 0 and time.monotonic() < deadline:
                text = output.read_text(errors="replace")
                if re.search(r"Decoded \d+ samples.*?\d+ channels.*?\d+ Hz", text):
                    sys.stdout.write(text)
                    sys.stderr.write(errors.read_text(errors="replace"))
                    return 0
                time.sleep(0.1)
            sys.stdout.write(output.read_text(errors="replace"))
            sys.stderr.write(errors.read_text(errors="replace"))
            sys.stderr.write(launch.stdout + launch.stderr)
            sys.stderr.write("Simulator decoder did not produce a complete result before the deadline\n")
            return launch.returncode or 1
        finally:
            try:
                subprocess.run(["xcrun", "simctl", "terminate", device, "org.auvemir.app"],
                               capture_output=True, timeout=2, check=False)
            except subprocess.TimeoutExpired:
                pass

if __name__ == "__main__":
    raise SystemExit(main())
