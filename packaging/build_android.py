#!/usr/bin/env python3
"""Build the shared Slint app with official Android SDK tools, without Gradle."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import urllib.request
import zipfile
from source_identity import source_digest

ROOT = Path(__file__).resolve().parents[1]
ARCHES = {
    "arm64": ("aarch64-linux-android", "arm64-v8a", "aarch64-linux-android", 16384),
    "x86_64": ("x86_64-linux-android", "x86_64", "x86_64-linux-android", 16384),
    "armv7": ("armv7-linux-androideabi", "armeabi-v7a", "armv7a-linux-androideabi", 4096),
    "arm32": ("armv7-linux-androideabi", "armeabi-v7a", "armv7a-linux-androideabi", 4096),
    "x86": ("i686-linux-android", "x86", "i686-linux-android", 4096),
    "x86_32": ("i686-linux-android", "x86", "i686-linux-android", 4096),
}

def run(args, env=None):
    subprocess.run([str(x) for x in args], cwd=ROOT, env=env, check=True)

def download(url, path, digest):
    if not path.is_file() or hashlib.sha256(path.read_bytes()).hexdigest() != digest:
        urllib.request.urlretrieve(url, path)
    if hashlib.sha256(path.read_bytes()).hexdigest() != digest:
        raise RuntimeError("Android helper checksum mismatch")

def verify_elf_alignment(path, min_align=16384):
    with path.open('rb') as source:
        header = source.read(64)
        if header[:4] != b'\x7fELF' or header[4] not in (1, 2) or header[5] not in (1, 2):
            raise RuntimeError(f'Invalid native ELF: {path.name}')
        endian = '<' if header[5] == 1 else '>'
        wide = header[4] == 2
        offset = struct.unpack_from(endian + ('Q' if wide else 'I'), header, 32 if wide else 28)[0]
        size, count = struct.unpack_from(endian + 'HH', header, 54 if wide else 42)
        layout = endian + ('IIQQQQQQ' if wide else 'IIIIIIII')
        expected = struct.calcsize(layout)
        if size < expected or count == 0:
            raise RuntimeError(f'Invalid program headers: {path.name}')
        loads = 0
        for i in range(count):
            source.seek(offset + i * size)
            segment = struct.unpack(layout, source.read(expected))
            if segment[0] == 1:
                loads += 1
                file_offset, address = (segment[2], segment[3]) if wide else (segment[1], segment[2])
                if segment[7] < min_align or (address - file_offset) % min_align:
                    raise RuntimeError(f'{path.name} has a load segment incompatible with {min_align} byte pages')
        if loads == 0:
            raise RuntimeError(f'No loadable segments: {path.name}')

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--arch",choices=ARCHES,default="arm64")
    parser.add_argument("--java-check",action="store_true")
    args=parser.parse_args()
    source_before=source_digest()
    sdk=Path(os.environ["ANDROID_HOME"]).resolve()
    ndk=Path(os.environ.get("ANDROID_NDK_ROOT",sdk/"ndk/android-ndk-r29")).resolve()
    tools=sdk/"build-tools/36.0.0";android=sdk/"platforms/android-36/android.jar"
    java=Path(os.environ["JAVA_HOME"])/"bin"
    target,abi,clang,page_align=ARCHES[args.arch]
    work=ROOT/"target"/f"android-package-{args.arch}";work.mkdir(parents=True,exist_ok=True)
    generated=work/"generated";classes=work/"classes";dex=work/"dex"
    for path in [generated,classes,dex]:
        if path.exists():shutil.rmtree(path)
        path.mkdir()
    env=os.environ.copy();env["PATH"]=str(java)+os.pathsep+env["PATH"]
    aar=work/"rustls-verifier.aar";kotlin=work/"kotlin-stdlib.jar"
    download("https://raw.githubusercontent.com/rustls/rustls-platform-verifier/maven-archive/android-release-support/maven/org/rustls/rustls-platform-verifier/0.2.0/rustls-platform-verifier-0.2.0.aar",aar,"aa021794230fbc2f0be355999e2cf67398dd563de066a2e17001ffbd0b69101b")
    download("https://repo.maven.apache.org/maven2/org/jetbrains/kotlin/kotlin-stdlib/2.2.21/kotlin-stdlib-2.2.21.jar",kotlin,"6558a3d233da56a20934b32159f9db5f86ed5816ef098f78a2c223dc6abb79dd")
    with zipfile.ZipFile(aar) as z:(work/"verifier.jar").write_bytes(z.read("classes.jar"))
    resources=work/"resources.zip";unsigned=work/"unsigned.apk"
    run([tools/"aapt2","compile","--dir",ROOT/"platforms/android/res","-o",resources],env)
    run([tools/"aapt2","link","-o",unsigned,"-I",android,"--manifest",ROOT/"platforms/android/AndroidManifest.xml","--java",generated,"--min-sdk-version","26","--target-sdk-version","36",resources],env)
    sources=list((ROOT/"platforms/android/java").rglob("*.java"))+list(generated.rglob("*.java"))
    run([java/"javac","--release","8","-classpath",android,"-d",classes,*sources],env)
    run([java/"jar","cf",work/"java.jar","-C",classes,"."],env)
    run([tools/"d8","--min-api","26","--lib",android,"--output",dex,work/"java.jar",work/"verifier.jar",kotlin],env)
    if args.java_check:
        print("Android manifest, Java and DEX checks passed")
        return
    toolchain=ndk/"toolchains/llvm/prebuilt/linux-x86_64"
    env.update({"ANDROID_NDK":str(ndk),"ANDROID_NDK_HOME":str(ndk),"ANDROID_NDK_ROOT":str(ndk),"ANDROID_ABI":abi,"ANDROID_PLATFORM":"android-26","ANDROID_JAR":str(android)})
    suffix=target.replace("-","_");cxx=toolchain/"bin"/(clang+"26-clang++");cc=toolchain/"bin"/(clang+"26-clang")
    clang_res=next((toolchain/"lib/clang").iterdir())
    env.update({f"CC_{suffix}":str(cc),f"CXX_{suffix}":str(cxx),f"AR_{suffix}":str(toolchain/"bin/llvm-ar"),f"CARGO_TARGET_{suffix.upper()}_LINKER":str(cc),f"CMAKE_TOOLCHAIN_FILE_{suffix}":str(ndk/"build/cmake/android.toolchain.cmake"),f"CXXFLAGS_{suffix}":"-fPIC",f"BINDGEN_EXTRA_CLANG_ARGS_{suffix}":f"--target={clang}26 --sysroot={toolchain}/sysroot -resource-dir={clang_res}"})
    run(["cargo","rustc","--locked","--release","-p","auvemir","--lib","--crate-type","cdylib","--target",target,"-j",env.get("CARGO_BUILD_JOBS","2")],env)
    cpp_target="arm-linux-androideabi" if args.arch in ("armv7", "arm32") else target
    cpp=toolchain/"sysroot/usr/lib"/cpp_target/"libc++_shared.so"
    native=ROOT/"target"/target/"release/libauvemir.so"
    verify_elf_alignment(native, min_align=page_align)
    verify_elf_alignment(cpp, min_align=page_align)
    with zipfile.ZipFile(unsigned,"a") as z:
        for path in dex.glob("*.dex"):z.write(path,path.name,compress_type=zipfile.ZIP_DEFLATED)
        z.write(native,f"lib/{abi}/libauvemir.so",compress_type=zipfile.ZIP_STORED)
        z.write(cpp,f"lib/{abi}/libc++_shared.so",compress_type=zipfile.ZIP_STORED)
    aligned=work/"aligned.apk"
    align_args=["-P","16","4"] if page_align==16384 else ["4"]
    run([tools/"zipalign","-f",*align_args,unsigned,aligned],env)
    configured=os.environ.get("AUVEMIR_ANDROID_KEYSTORE")
    keystore=Path(configured) if configured else ROOT/"target/android-development.keystore"
    alias=os.environ.get("AUVEMIR_ANDROID_KEY_ALIAS","auvemir")
    if not configured:
        env["AUVEMIR_ANDROID_KEYSTORE_PASSWORD"]="android"
        if not keystore.is_file():
            run([java/"keytool","-genkeypair","-keystore",keystore,"-alias",alias,"-keyalg","RSA","-keysize","3072","-validity","3650","-dname","CN=Auvemir Development","-storepass","android","-keypass","android"],env)
        keystore.chmod(0o600)
    elif not env.get("AUVEMIR_ANDROID_KEYSTORE_PASSWORD"):
        raise RuntimeError("Set AUVEMIR_ANDROID_KEYSTORE_PASSWORD for the supplied keystore")
    dist=ROOT/"dist";dist.mkdir(exist_ok=True)
    label="release" if configured else "development"
    output=dist/f"Auvemir-android-{args.arch}-{label}.apk"
    run([tools/"apksigner","sign","--ks",keystore,"--ks-key-alias",alias,"--ks-pass","env:AUVEMIR_ANDROID_KEYSTORE_PASSWORD","--out",output,aligned],env)
    run([tools/"apksigner","verify","--verbose",output],env)
    run([tools/"zipalign","-c",*align_args,output],env)
    with output.open('rb') as source:digest=hashlib.file_digest(source,'sha256').hexdigest()
    if source_digest()!=source_before:raise RuntimeError("Application/build inputs changed during the build; rebuild before distribution")
    align_tag = "16k" if page_align == 16384 else "4k"
    report={"artifact":output.name,"bytes":output.stat().st_size,"sha256":digest,"source_sha256":source_before,"abi":abi,"development_key":not bool(configured),"signature_verified":True,f"zip_{align_tag}_alignment_verified":True,f"elf_{align_tag}_alignment_verified":True,"device_tested":False}
    (ROOT/f"docs/android-{args.arch}-build-results.json").write_text(json.dumps(report,indent=2)+"\n")
    print(json.dumps(report,indent=2))

if __name__=="__main__":main()
