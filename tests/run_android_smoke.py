#!/usr/bin/env python3
"""Boot an isolated emulator and verify real native playback and Activity replacement.
Requires SDK emulator and the default Android 28 x86_64 system image; no KVM required.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import tempfile
import subprocess
import time
ROOT=Path(__file__).resolve().parents[1]
def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--api',type=int,default=28,help='Android API level for the installed emulator image')
    parser.add_argument('--image-tag',choices=['default','google_apis'],default='default')
    parser.add_argument('--accel',choices=['auto','on','off'],default='auto',help='Use KVM when available; fall back to software emulation')
    parser.add_argument('--apk',type=Path,required=True)
    parser.add_argument('--build-report',type=Path)
    parser.add_argument('--build-log',type=Path)
    parser.add_argument('--tests',type=Path,default=ROOT/'target/android-smoke/Auvemir-tests.apk')
    parser.add_argument('--keep-emulator',action='store_true',help='Keep the VM for interactive debugging in this command session')
    parser.add_argument('--existing',action='store_true',help='Use emulator-5554 already running in this command session')
    parser.add_argument('--reuse-avd',type=Path,help='Reuse a retained test-owned AVD directory without wiping it')
    args=parser.parse_args()
    sdk=Path(os.environ['ANDROID_HOME']);image=sdk/f'system-images/android-{args.api}/{args.image_tag}/x86_64'
    if not args.existing and not (image/'system.img').is_file():raise RuntimeError(f'Install system-images;android-{args.api};{args.image_tag};x86_64 first')
    work=ROOT/'target/android-runtime';work.mkdir(parents=True,exist_ok=True)
    if args.reuse_avd:
        home=args.reuse_avd.resolve()
        if home.parent!=work.resolve() or not home.name.startswith('avd-') or not (home/'AuvemirSmoke.ini').is_file():raise ValueError('Only retained Auvemir test AVDs may be reused')
    else:home=Path(tempfile.mkdtemp(prefix='avd-',dir=work))
    avd=home/'AuvemirSmoke.avd';avd.mkdir(parents=True,exist_ok=True)
    (home/'AuvemirSmoke.ini').write_text(f'path={avd}\ntarget=android-{args.api}\n')
    (avd/'config.ini').write_text(f'''AvdId=AuvemirSmoke
avd.ini.displayname=Auvemir verification
image.sysdir.1={image}/
hw.cpu.arch=x86_64
hw.cpu.ncore=1
hw.ramSize=768
hw.lcd.width=480
hw.lcd.height=854
hw.lcd.density=200
hw.gpu.enabled=yes
hw.gpu.mode=swiftshader
disk.dataPartition.size=512M
hw.audioInput=yes
hw.audioOutput=yes
hw.keyboard=yes
hw.mainKeys=no
showDeviceFrame=no
tag.id={args.image_tag}
PlayStore.enabled=false
''')
    accelerated=args.accel=='on' or args.accel=='auto' and os.access('/dev/kvm',os.R_OK|os.W_OK)
    env=os.environ.copy();env['ANDROID_AVD_HOME']=str(home);env['ANDROID_I_WANT_MY_TCG']='yes';env['QT_QPA_PLATFORM']='offscreen'
    adb=[str(sdk/'platform-tools/adb'),'-P','5037']
    def call(command,timeout=30,check=True,binary=False):
        return subprocess.run(command,env=env,capture_output=True,text=not binary,timeout=timeout,check=check,stdin=subprocess.DEVNULL)
    log=(work/'emulator.log').open('a' if args.existing else 'w')
    emulator=None if args.existing else subprocess.Popen([str(sdk/'emulator/emulator'),'-avd','AuvemirSmoke','-port','5554','-accel','on' if accelerated else 'off','-cores','2' if accelerated else '1','-memory','768','-no-window','-no-boot-anim','-no-snapshot','-no-snapstorage','-no-cache','-partition-size','1024',*(() if args.reuse_avd else ('-wipe-data',)),'-no-audio','-no-metrics','-gpu','swiftshader','-feature','-Vulkan,-QuickbootFileBacked','-camera-back','none','-camera-front','none'],env=env,stdout=log,stderr=subprocess.STDOUT)
    result={'success':False,'physical_device':False,'system_image':f'Android {args.api} {args.image_tag} x86_64','acceleration':'KVM' if accelerated else 'software','scope':'cold media-browser/native playback, OS audio focus, Activity replacement and microphone pause/resume/finalization','stage':'boot','app_process_started':False}
    try:
        call(adb+['start-server']);deadline=time.monotonic()+600
        while time.monotonic()<deadline:
            if emulator is not None and emulator.poll() is not None:raise RuntimeError('Emulator exited before boot; inspect target/android-runtime/emulator.log')
            try:
                boot=call(adb+['-s','emulator-5554','shell','getprop','sys.boot_completed'],timeout=5,check=False)
                if boot.stdout.strip()=='1':break
            except subprocess.TimeoutExpired:
                pass
            time.sleep(2)
        else:raise TimeoutError('Software emulator did not boot within ten minutes')
        result['stage']='waiting-for-build'
        print('Emulator booted; waiting for a verified application package',flush=True)
        deadline=time.monotonic()+2400
        while time.monotonic()<deadline:
            ready=args.apk.is_file()
            if ready and args.build_report:
                try:
                    report=json.loads(args.build_report.read_text())
                    with args.apk.open('rb') as source:ready=hashlib.file_digest(source,'sha256').hexdigest()==report['sha256']
                except (OSError,ValueError,KeyError):ready=False
            if ready:break
            if args.build_log and args.build_log.is_file():
                tail=args.build_log.read_text(errors='replace')[-4000:]
                if 'Traceback (most recent call last)' in tail:raise RuntimeError('Application build failed; inspect its build log')
            time.sleep(3)
        else:raise TimeoutError('No verified application package became available')
        with args.apk.open('rb') as source:result['apk_sha256']=hashlib.file_digest(source,'sha256').hexdigest()
        if args.build_report:result['source_sha256']=report['source_sha256']
        result['stage']='installation'
        device=adb+['-s','emulator-5554']
        call(device+['root'],check=False);call(device+['wait-for-device'],timeout=30)
        call(device+['shell','settings','put','global','window_animation_scale','0'],check=False)
        call(device+['shell','settings','put','global','transition_animation_scale','0'],check=False)
        call(device+['shell','settings','put','global','animator_duration_scale','0'],check=False)
        print('Installing the application package without streaming',flush=True)
        call(device+['install','--no-streaming','-r',str(args.apk)],timeout=300)
        call(device+['install','--no-streaming','-r',str(args.tests)],timeout=120)
        call(device+['shell','pm','grant','org.auvemir.app','android.permission.RECORD_AUDIO'],check=False)
        call(device+['shell','pm','grant','org.auvemir.app','android.permission.POST_NOTIFICATIONS'],check=False)
        result['stage']='instrumentation'
        call(device+['logcat','-c']);print('Installed app and separate instrumentation runner',flush=True)
        test=call(device+['shell','am','instrument','-w','org.auvemir.app.tests/org.auvemir.tests.Smoke'],timeout=300,check=False)
        (work/'instrumentation.txt').write_text(test.stdout+test.stderr)
        found=re.search(r'^INSTRUMENTATION_RESULT: report=(.*)$',test.stdout,re.MULTILINE)
        if not found:raise RuntimeError('Instrumentation did not return a report')
        observed=json.loads(found.group(1));result.update(observed);result['app_process_started']=True
        if observed.get('success'):result['stage']='completed'
        windows=call(device+['shell','dumpsys','window','windows'],timeout=30,check=False).stdout
        result['emulator_system_ui_unresponsive']='Application Not Responding: com.android.systemui' in windows
        result['full_ui_interaction_validated']=False
        print(json.dumps(result,indent=2),flush=True)
        if observed.get('success'):
            screenshot=call(device+['exec-out','cat','/data/user/0/org.auvemir.app/files/smoke-ui.png'],binary=True,check=False)
            if screenshot.returncode==0 and screenshot.stdout.startswith(b'\x89PNG\r\n\x1a\n'):(ROOT/'docs/phone-runtime.png').write_bytes(screenshot.stdout)
        (work/'logcat.txt').write_text(call(device+['logcat','-d','-v','threadtime'],timeout=30,check=False).stdout)
        if not observed.get('success'):raise RuntimeError(observed.get('error','Android runtime verification failed'))
    except Exception as error:
        result['success']=False;result['error']=str(error)
        for field in ('stdout','stderr'):
            value=getattr(error,field,None)
            if value:result[field]=value.decode(errors='replace') if isinstance(value,bytes) else value
        try:(work/'logcat.txt').write_text(call(adb+['-s','emulator-5554','logcat','-d','-v','threadtime'],timeout=10,check=False).stdout)
        except (subprocess.SubprocessError,OSError):pass
        raise
    finally:
        (ROOT/'docs/android-runtime-results.json').write_text(json.dumps(result,indent=2)+'\n')
        if not args.keep_emulator and not args.existing:
            try:call(adb+['-s','emulator-5554','emu','kill'],timeout=5,check=False)
            except (subprocess.SubprocessError,OSError):pass
            if emulator is not None and emulator.poll() is None:
                emulator.terminate()
                try:emulator.wait(timeout=10)
                except subprocess.TimeoutExpired:emulator.kill();emulator.wait(timeout=10)
            try:call(adb+['kill-server'],timeout=5,check=False)
            except (subprocess.SubprocessError,OSError):pass
            shutil.rmtree(home,ignore_errors=True)
        elif args.existing:
            shutil.rmtree(home,ignore_errors=True)
        else:
            print('Emulator retained for this command session at emulator-5554',flush=True)
        log.close()
if __name__=='__main__':main()
