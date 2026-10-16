#!/usr/bin/env python3
"""Extract real Linux packages and compare their ELF with the compiled release."""
import hashlib
import json
from pathlib import Path
import platform
import subprocess
import sys
import tarfile
import tempfile
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'packaging'))
from source_identity import source_digest

def digest(path):
    with path.open('rb') as file:return hashlib.file_digest(file,'sha256').hexdigest()

def main():
    arch=platform.machine();deb={'x86_64':'amd64','aarch64':'arm64'}[arch]
    dist=ROOT/'dist';expected=digest(ROOT/'target/release/auvemir');checks=[]
    packages=[('portable',f'auvemir-0.1.0-linux-{arch}.tar.gz'),('deb',f'auvemir_0.1.0_{deb}.deb'),('arch',f'auvemir-0.1.0-1-{arch}.pkg.tar.zst'),('rpm',f'auvemir-0.1.0-1.{arch}.rpm')]
    with tempfile.TemporaryDirectory(prefix='auvemir-package-check-') as directory:
        work=Path(directory)
        for kind,name in packages:
            output=work/kind;output.mkdir();package=dist/name
            if kind=='portable':
                with tarfile.open(package) as archive:archive.extractall(output,filter='data')
            elif kind=='deb':subprocess.run(['dpkg-deb','-x',str(package),str(output)],check=True)
            elif kind=='arch':
                with subprocess.Popen(['zstd','-q','-d','-c',str(package)],stdout=subprocess.PIPE) as child:
                    with tarfile.open(fileobj=child.stdout,mode='r|') as archive:archive.extractall(output,filter='data')
                    if child.wait()!=0:raise RuntimeError('Arch package decompression failed')
            else:
                subprocess.run(['rpmkeys','--checksig','--nosignature',str(package)],check=True)
                with subprocess.Popen(['rpm2cpio',str(package)],stdout=subprocess.PIPE) as child:
                    subprocess.run(['cpio','-id','--quiet','--no-absolute-filenames'],cwd=output,stdin=child.stdout,check=True)
                    if child.wait()!=0:raise RuntimeError('RPM payload extraction failed')
            binaries=list(output.rglob('usr/bin/auvemir'))
            if len(binaries)!=1 or digest(binaries[0])!=expected:raise RuntimeError('Packaged ELF differs from the release: '+name)
            if not list(output.rglob('THIRD_PARTY_NOTICES.md')):raise RuntimeError('Missing third-party notices: '+name)
            checks.append({'package':name,'container_valid':True,'binary_matches_release':True,'notice_present':True})
        app=dist/f'Auvemir-0.1.0-{arch}.AppImage';output=work/'appimage';output.mkdir()
        subprocess.run([str(app),'--appimage-extract'],cwd=output,check=True,stdout=subprocess.DEVNULL)
        if digest(output/'squashfs-root/usr/bin/auvemir')!=expected:raise RuntimeError('AppImage ELF differs from the release')
        fixture=work/'tone.wav';subprocess.run(['ffmpeg','-loglevel','error','-f','lavfi','-i','sine=frequency=440:duration=1','-y',str(fixture)],check=True)
        response=subprocess.run([str(output/'squashfs-root/AppRun'),'--decode',str(fixture)],check=True,capture_output=True,text=True)
        if 'Decoded 44100 samples' not in response.stdout:raise RuntimeError('Extracted AppImage launcher did not decode the fixture')
        checks.append({'package':app.name,'container_valid':True,'binary_matches_release':True,'extracted_launcher_decode_passed':True})
    report={'source_sha256':source_digest(),'binary_sha256':expected,'packages':checks,'system_installation_tested':False,'physical_device_tested':False}
    (ROOT/'docs/linux-build-results.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
if __name__=='__main__':main()
