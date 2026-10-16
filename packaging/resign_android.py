#!/usr/bin/env python3
"""Re-sign a CI APK with an existing identity without changing code or resources."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import zipfile

from build_android import verify_elf_alignment


def payload_digest(path):
    digest = hashlib.sha256()
    with zipfile.ZipFile(path) as archive:
        names = archive.namelist()
        if len(names) != len(set(names)):
            raise ValueError("APK contains duplicate entries")
        for name in sorted(names):
            upper = name.upper()
            if upper.startswith("META-INF/") and (upper == "META-INF/MANIFEST.MF" or upper.endswith((".SF", ".RSA", ".DSA", ".EC"))):
                continue
            info = archive.getinfo(name)
            encoded = name.encode()
            digest.update(len(encoded).to_bytes(8, "little"))
            digest.update(encoded)
            digest.update(info.file_size.to_bytes(8, "little"))
            with archive.open(info) as source:
                while block := source.read(1024 * 1024):
                    digest.update(block)
    return digest.hexdigest()


def run(command):
    return subprocess.run([str(x) for x in command], check=True, capture_output=True, text=True).stdout


def signer(tools, apk):
    output = run([tools / "apksigner", "verify", "--verbose", "--print-certs", apk])
    digests = re.findall(r"Signer #\d+ certificate SHA-256 digest: ([a-fA-F0-9]{64})", output)
    if len(digests) != 1:
        raise ValueError("Expected exactly one verified APK signer")
    return digests[0].lower()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("apk", type=Path)
    parser.add_argument("--build-tools", required=True, type=Path)
    parser.add_argument("--keystore", required=True, type=Path)
    parser.add_argument("--alias", default="auvemir")
    parser.add_argument("--password-env", default="AUVEMIR_ANDROID_KEYSTORE_PASSWORD")
    parser.add_argument("--match-apk", required=True, type=Path, help="Previously distributed APK whose signer must be retained")
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--report", required=True, type=Path)
    args = parser.parse_args()
    if not os.environ.get(args.password_env):
        raise ValueError("Set the named keystore password environment variable")
    if args.output.exists() or args.report.exists():
        raise FileExistsError("Output APK and report must use new paths")
    expected_signer = signer(args.build_tools, args.match_apk)
    signer(args.build_tools, args.apk)
    before = payload_digest(args.apk)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="auvemir-sign-", dir=args.output.parent) as directory:
        work = Path(directory)
        signed = work / "signed.apk"
        run([args.build_tools / "apksigner", "sign", "--ks", args.keystore,
             "--ks-key-alias", args.alias, "--ks-pass", "env:" + args.password_env,
             "--out", signed, args.apk])
        observed_signer = signer(args.build_tools, signed)
        if observed_signer != expected_signer:
            raise ValueError("Signing identity differs from the previously distributed APK")
        run([args.build_tools / "zipalign", "-c", "-P", "16", "4", signed])
        if payload_digest(signed) != before:
            raise ValueError("Re-signing changed APK code or resources")
        with zipfile.ZipFile(signed) as archive:
            libraries = [info for info in archive.infolist() if info.filename.startswith("lib/") and info.filename.endswith(".so")]
            if not libraries:
                raise ValueError("APK has no native libraries")
            for info in libraries:
                native = work / "native.so"
                with archive.open(info) as source, native.open("wb") as output:
                    shutil.copyfileobj(source, output, 1024 * 1024)
                verify_elf_alignment(native)
        with signed.open("rb") as source:
            checksum = hashlib.file_digest(source, "sha256").hexdigest()
        with args.apk.open("rb") as source:
            original_checksum = hashlib.file_digest(source, "sha256").hexdigest()
        report = {"artifact": args.output.name, "sha256": checksum,
                  "ci_input_sha256": original_checksum, "payload_sha256": before,
                  "code_and_resources_preserved": True,
                  "signer_matches_previous_install": True, "signer_sha256": observed_signer,
                  "signature_verified": True, "zip_16k_alignment_verified": True,
                  "elf_16k_alignment_verified": True, "physical_device_tested": False}
        # Atomic, non-overwriting publication; all verification happens first.
        os.link(signed, args.output)
    try:
        with args.report.open("x") as output:
            output.write(json.dumps(report, indent=2) + "\n")
    except Exception:
        args.output.unlink()
        raise
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
