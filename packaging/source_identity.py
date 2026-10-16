"""Identify application/build inputs without including generated reports or caches."""
import hashlib
import os
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
EXCLUDED = {'.git', 'target', 'dist', 'vendor', '__pycache__', 'docs', '.build', '.swiftpm', 'xtool'}

def source_digest():
    digest = hashlib.sha256()
    paths = []
    for directory, directories, names in os.walk(ROOT):
        directories[:] = [name for name in directories if name not in EXCLUDED]
        paths.extend(Path(directory) / name for name in names if name not in EXCLUDED and name != ".DS_Store")
    paths.sort(key=lambda path: path.relative_to(ROOT).as_posix())
    for path in paths:
        name = path.relative_to(ROOT).as_posix().encode()
        digest.update(len(name).to_bytes(8, 'little'))
        digest.update(name)
        digest.update(path.stat().st_size.to_bytes(8, 'little'))
        with path.open('rb') as source:
            while block := source.read(1024 * 1024):
                digest.update(block)
    return digest.hexdigest()
