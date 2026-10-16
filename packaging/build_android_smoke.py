#!/usr/bin/env python3
"""Build a separate same-signature instrumentation APK; no test runner is shipped in Auvemir."""
import os
from pathlib import Path
import subprocess
import zipfile
ROOT=Path(__file__).resolve().parents[1]
def run(args,env):subprocess.run([str(a) for a in args],cwd=ROOT,env=env,check=True)
def main():
    sdk=Path(os.environ['ANDROID_HOME']);tools=sdk/'build-tools/36.0.0';android=sdk/'platforms/android-36/android.jar';java=Path(os.environ['JAVA_HOME'])/'bin'
    work=ROOT/'target/android-smoke';work.mkdir(parents=True,exist_ok=True);classes=work/'classes';dex=work/'dex'
    classes.mkdir(exist_ok=True);dex.mkdir(exist_ok=True)
    manifest=work/'AndroidManifest.xml';manifest.write_text('''<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="org.auvemir.app.tests" android:versionCode="1" android:versionName="1"><uses-sdk android:minSdkVersion="26" android:targetSdkVersion="36"/><application android:label="Auvemir verification" android:debuggable="true"/><instrumentation android:name="org.auvemir.tests.Smoke" android:targetPackage="org.auvemir.app" android:functionalTest="true"/></manifest>''')
    env=os.environ.copy();env['PATH']=str(java)+os.pathsep+env['PATH']
    unsigned=work/'unsigned.apk';aligned=work/'aligned.apk';output=work/'Auvemir-tests.apk'
    run([tools/'aapt2','link','-o',unsigned,'-I',android,'--manifest',manifest,'--min-sdk-version','26','--target-sdk-version','36'],env)
    run([java/'javac','--release','8','-classpath',android,'-d',classes,ROOT/'platforms/android/tests/Smoke.java'],env)
    run([java/'jar','cf',work/'tests.jar','-C',classes,'.'],env)
    run([tools/'d8','--min-api','26','--lib',android,'--output',dex,work/'tests.jar'],env)
    with zipfile.ZipFile(unsigned,'a') as z:
        for path in dex.glob('*.dex'):z.write(path,path.name,compress_type=zipfile.ZIP_DEFLATED)
    run([tools/'zipalign','-f','-P','16','4',unsigned,aligned],env)
    configured=os.environ.get('AUVEMIR_ANDROID_KEYSTORE');keystore=Path(configured) if configured else ROOT/'target/android-development.keystore'
    if not keystore.is_file():raise RuntimeError('Build the application first to establish its signing identity')
    if not configured:env['AUVEMIR_ANDROID_KEYSTORE_PASSWORD']='android'
    if not env.get('AUVEMIR_ANDROID_KEYSTORE_PASSWORD'):raise RuntimeError('Supply the same signing password used for Auvemir')
    run([tools/'apksigner','sign','--ks',keystore,'--ks-key-alias',env.get('AUVEMIR_ANDROID_KEY_ALIAS','auvemir'),'--ks-pass','env:AUVEMIR_ANDROID_KEYSTORE_PASSWORD','--out',output,aligned],env)
    run([tools/'apksigner','verify',output],env);print(output)
if __name__=='__main__':main()
