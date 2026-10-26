#!/usr/bin/env python3
"""Exercise real native journeys and render responsive/DPI configurations on Linux."""
import argparse, hashlib, json, os, sqlite3, subprocess, sys, tempfile, time
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/"tools"))
from PIL import Image
from capture_screenshots import capture
from create_demo_library import create
from native_session import ROOT, display, input_command, profile_environment, source_digest, wait_for

def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument("binary",type=Path);parser.add_argument("--report",type=Path,default=ROOT/"docs/validation/desktop.json");args=parser.parse_args();binary=args.binary.resolve();checks=[]
    for flag in ["--help","--version"]:
        env=os.environ.copy();env.pop("DISPLAY",None)
        result=subprocess.run([str(binary),flag],env=env,capture_output=True,text=True,timeout=5);assert result.returncode==0 and "Auvemir" in result.stdout;checks.append(flag+" without display")
    with tempfile.TemporaryDirectory(prefix="auvemir-desktop-test-") as directory, display() as session:
        folder=Path(directory);library=create(folder/"media");env=profile_environment(session,folder/"profile");db=folder/"profile/auvemir.sqlite"
        capture(binary,env,folder/"import.png",argument=library)
        def scalar(sql, parameters=()):
            try:
                with sqlite3.connect(db) as conn:
                    result=conn.execute(sql,parameters).fetchone();return result[0] if result else None
            except sqlite3.Error: return None
        assert scalar("SELECT count(*) FROM media")==12;checks.append("actual file import and persistence")
        with (folder/"journey.log").open("w") as log:
            app=subprocess.Popen([str(binary)],env=env,stdout=log,stderr=subprocess.STDOUT)
            try:
                def window_id():
                    result=subprocess.run(["xdotool","search","--onlyvisible","--name","^Auvemir$"],env=env,capture_output=True,text=True)
                    return result.stdout.strip().splitlines()[-1] if result.returncode==0 else None
                window=wait_for(window_id);input_command(env,"windowsize","--sync",window,1180,780);input_command(env,"windowfocus",window);time.sleep(.4)
                def key(value): input_command(env,"windowfocus","--sync",window);input_command(env,"key","--clearmodifiers",value);time.sleep(.25)
                def click(x,y): input_command(env,"windowfocus","--sync",window);input_command(env,"mousemove","--window",window,x,y);time.sleep(.1);input_command(env,"click",1);time.sleep(.25)
                key("ctrl+2")
                # Matte 0.2 layout: 42px controls move sorting and row actions lower.
                click(470,356);key("Home");key("Down");key("Down");key("Escape");click(450,100)
                wait_for(lambda: scalar("SELECT value FROM settings WHERE key='library-sort'")=="2");checks.append("native sort control persists artist order")
                click(1028,480);wait_for(lambda: scalar("SELECT count(*) FROM media WHERE favorite=1")==1);checks.append("native favorite action")
                click(1072,480);wait_for(lambda: len(json.loads(scalar("SELECT value FROM settings WHERE key='queue'") or "[[],0]")[0]) == 1);checks.append("native add to queue")
                click(400,480);wait_for(lambda: scalar("SELECT count(*) FROM history")>0);checks.append("actual decode/output pipeline through null ALSA sink")
                key("ctrl+comma");click(500,194);key("Home");key("Down");key("Escape");click(450,100)
                click(330,488);wait_for(lambda: scalar("SELECT value FROM settings WHERE key='appearance-light'")=="true")
                click(420,652);wait_for(lambda: scalar("SELECT value FROM settings WHERE key='motion-reduced'")=="true");checks.append("native light/reduced-motion preference controls")
                key("ctrl+slash");key("Escape");key("ctrl+f");input_command(env,"type","--clearmodifiers","Drift");key("Return");assert app.poll() is None;checks.append("keyboard guide/escape/search shortcut sequence remains responsive")
                key("ctrl+q");assert app.wait(timeout=5)==0;checks.append("keyboard quit and normal worker cleanup")
            finally:
                if app.poll() is None: app.kill();app.wait()
        capture(binary,env,folder/"restart.png",page=11,settings=1)
        assert scalar("SELECT value FROM settings WHERE key='appearance-light'")=="true"
        assert scalar("SELECT value FROM settings WHERE key='appearance-theme'")=="white"
        assert scalar("SELECT value FROM settings WHERE key='motion-reduced'")=="true"
        assert scalar("SELECT value FROM settings WHERE key='library-sort'")=="2"
        assert Image.open(folder/"restart.png").getpixel((1150,100))[:3]==(255,255,255)
        checks.append("saved appearance, motion and sort survive restart")
        for width,height,scale in [(320,640,1),(390,844,1),(720,720,1),(1180,780,1),(1180,780,1.5),(1440,900,1)]:
            current=env.copy();current["SLINT_SCALE_FACTOR"]=str(scale);out=folder/f"viewport-{width}-{scale}.png"
            capture(binary,current,out,page=1,width=width,height=height)
            assert Image.open(out).size==(round(width*scale),round(height*scale));checks.append(f"viewport {width}x{height} at {scale}x DPI")
        # Render every destination and settings group at the minimum supported width.
        for page in [0,2,3,4,5,6,7,8,9,10]:
            out=folder/f"phone-page-{page}.png"
            capture(binary,env,out,page=page,width=320,height=640)
            assert Image.open(out).size==(320,640);checks.append(f"minimum-width page {page} render")
        for section in range(7):
            out=folder/f"phone-settings-{section}.png"
            capture(binary,env,out,page=11,settings=section,width=320,height=640)
            assert Image.open(out).size==(320,640);checks.append(f"minimum-width settings group {section} render")
        capture(binary,env,folder/"phone-player.png",page=1,player=True,width=320,height=640)
        checks.append("minimum-width expanded player render")
        bad=env.copy();bad.update(AUVEMIR_SCREENSHOT=str(folder/"missing/output.png"),AUVEMIR_SCREENSHOT_DELAY_MS="100")
        result=subprocess.run([str(binary)],env=bad,capture_output=True,text=True,timeout=10)
        assert result.returncode!=0 and "Could not save capture" in result.stderr+result.stdout;checks.append("capture write failure returns nonzero after cleanup")
    report={"source_sha256":source_digest(),"binary_sha256":hashlib.sha256(binary.read_bytes()).hexdigest(),"result":"passed","checks":checks,"limitations":["Linux software renderer and null audio sink only","No physical mobile, microphone, receiver or native secure-store certification","Screen-reader and dynamic-type audit remains manual"]}
    args.report.parent.mkdir(parents=True,exist_ok=True);args.report.write_text(json.dumps(report,indent=2)+"\n");print(json.dumps(report,indent=2))
if __name__=="__main__":main()
