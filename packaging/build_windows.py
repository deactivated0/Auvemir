#!/usr/bin/env python3
"""Build a native Windows executable, portable ZIP and per-user NSIS installer."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tomllib
import zipfile
from source_identity import source_digest

ROOT = Path(__file__).resolve().parents[1]
VERSION = tomllib.loads((ROOT / 'Cargo.toml').read_text())['workspace']['package']['version']
RUNTIMES = ("libstdc++-6.dll", "libgcc_s_seh-1.dll", "libwinpthread-1.dll")

def run(args, env=None, cwd=ROOT):
    subprocess.run([str(x) for x in args], cwd=cwd, env=env, check=True)

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--target')
    parser.add_argument('--skip-build', action='store_true')
    parser.add_argument('--runtime-dir', action='append', type=Path, default=[])
    parser.add_argument('--makensis', default=shutil.which('makensis'))
    args = parser.parse_args()
    source_before = source_digest()
    host = subprocess.check_output(['rustc', '-vV'], text=True).split('host: ', 1)[1].splitlines()[0]
    target = args.target or host
    if 'windows' not in target:
        parser.error('Run on Windows or specify a Windows --target and cross toolchain')
    arch = 'arm64' if target.startswith('aarch64') else 'x86_64'
    if arch != 'x86_64':
        parser.error('The NSIS package currently targets Windows x86_64')
    env = os.environ.copy()
    if target.endswith('msvc'):
        env['RUSTFLAGS'] = env.get('RUSTFLAGS', '') + ' -C target-feature=+crt-static'
    if not args.skip_build:
        run(['cargo', 'build', '--locked', '--release', '-p', 'auvemir', '--target', target, '-j', env.get('CARGO_BUILD_JOBS', '2')], env)
    binary = ROOT / 'target' / target / 'release' / 'auvemir.exe'
    if not binary.is_file():
        raise FileNotFoundError(binary)
    bundle = ROOT / 'dist' / 'Auvemir'
    bundle.mkdir(parents=True, exist_ok=True)
    for old in bundle.iterdir():
        if old.is_file() and old.suffix.lower() == '.dll':
            old.unlink()
    shutil.copy2(binary, bundle / binary.name)
    for name in ['LICENSE', 'README.md', 'THIRD_PARTY_NOTICES.md']:
        shutil.copy2(ROOT / name, bundle / name)
    included = []
    notice = bundle / 'THIRD-PARTY-MINGW.txt'
    if notice.exists():
        notice.unlink()
    if target.endswith('gnu'):
        shutil.copy2(ROOT / 'packaging/native-notices/mingw-runtime.txt', notice)
        for name in RUNTIMES:
            candidates = [p / name for p in args.runtime_dir if (p / name).is_file()]
            if not candidates:
                raise RuntimeError(f'Provide --runtime-dir containing {name} for the GNU build')
            shutil.copy2(candidates[0], bundle / name)
            included.append(name)
    archive = ROOT / 'dist' / f'Auvemir-windows-{arch}.zip'
    with zipfile.ZipFile(archive, 'w', zipfile.ZIP_DEFLATED, compresslevel=6) as output:
        for path in sorted(bundle.iterdir()):
            if path.is_file():
                output.write(path, Path('Auvemir') / path.name)
    if not args.makensis:
        raise RuntimeError('Install NSIS or provide --makensis to build the installer')
    prefix = '/' if os.name == 'nt' else '-'
    run([args.makensis, prefix + 'DAPP_BINARY=' + str(binary), prefix + 'DAPP_VERSION=' + VERSION, 'windows.nsi'], env, ROOT / 'packaging')
    installer = ROOT / 'dist' / f'Auvemir-{VERSION}-windows-x86_64-Setup.exe'
    artifacts = []
    for path in [archive, installer]:
        with path.open('rb') as source:
            digest = hashlib.file_digest(source, 'sha256').hexdigest()
        artifacts.append({"artifact":path.name, "bytes":path.stat().st_size,"sha256":digest})
    if source_digest() != source_before:
        raise RuntimeError('Application/build inputs changed during the build; rebuild before distribution')
    report = {"target":target,"source_sha256":source_before,"artifacts":artifacts,"runtime_libraries":included,"device_tested":False,"signed":False}
    (ROOT / 'docs' / 'windows-build-results.json').write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps(report, indent=2))

if __name__ == '__main__':
    main()
