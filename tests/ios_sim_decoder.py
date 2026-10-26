#!/usr/bin/env python3
"""Run the installed app's decoder as a synchronous simulator process."""
import os
from pathlib import Path
import shutil
import subprocess
import sys

def main(arguments=None):
    arguments = sys.argv[1:] if arguments is None else arguments
    if len(arguments) != 2 or arguments[0] != "--decode":
        raise SystemExit("Usage: ios_sim_decoder.py --decode AUDIO")
    source = Path(arguments[1])
    destination = Path(os.environ["AUVEMIR_SIMULATOR_DATA"]) / "Documents" / source.name
    destination.parent.mkdir(exist_ok=True)
    shutil.copyfile(source, destination)
    result = subprocess.run(
        ["xcrun", "simctl", "spawn", os.environ["AUVEMIR_SIMULATOR_ID"],
         os.environ["AUVEMIR_SIMULATOR_EXECUTABLE"], "--decode", str(destination)],
        capture_output=True, text=True, timeout=15,
        env={**os.environ, "SIMCTL_CHILD_RUST_LOG": os.environ.get("RUST_LOG", "error")},
    )
    sys.stdout.write(result.stdout)
    sys.stderr.write(result.stderr)
    return result.returncode

if __name__ == "__main__":
    raise SystemExit(main())
