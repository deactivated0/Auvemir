"""Verify and unpack the reviewed Auvemir checkpoint for private CI jobs."""
import hashlib
import json
import os
from pathlib import Path,PurePosixPath
import stat
import sys
import zipfile
EXPECTED_SHA256='eb9159511cbdd8d96f6e1e2123da13e4c35432f14df6ab8676b263f0612ade78'
archive=Path('Auvemir-Source.zip')
with archive.open('rb') as source:observed=hashlib.file_digest(source,'sha256').hexdigest()
if observed!=EXPECTED_SHA256:raise RuntimeError('Source checkpoint SHA-256 differs from the reviewed input')
with zipfile.ZipFile(archive) as source:
    names=set()
    for info in source.infolist():
        path=PurePosixPath(info.filename)
        if path.is_absolute() or '..' in path.parts or not path.parts or path.parts[0]!='Auvemir':raise RuntimeError('Unexpected archive path')
        if stat.S_ISLNK(info.external_attr>>16):raise RuntimeError('Source archive may not introduce symbolic links')
        if info.filename.casefold() in names:raise RuntimeError('Duplicate archive path')
        names.add(info.filename.casefold())
    if source.testzip() is not None:raise RuntimeError('Source archive CRC verification failed')
    source.extractall('source')
root=Path('source/Auvemir')
for pattern in ('*results*.json','*codecs*.json','phone-runtime.png','ios-simulator.png'):
    for path in (root/'docs').glob(pattern):path.unlink()
sys.path.insert(0,str((root/'packaging').resolve()))
from source_identity import source_digest
report={'archive_sha256':observed,'source_sha256':source_digest(),'github_commit':os.environ.get('GITHUB_SHA'),'runner_os':os.environ.get('RUNNER_OS'),'current_job_reports_only':True,'full_original_brief_complete':False}
(root/'docs/validation-source.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report,indent=2))
