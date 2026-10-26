#!/usr/bin/env python3
"""Capture actual Slint windows, synthetic owned media, and real navigation motion."""
import argparse, hashlib, json, os, sqlite3, subprocess, tempfile, time
from pathlib import Path
from PIL import Image
from create_demo_library import create
from native_session import ROOT, display, input_command, profile_environment, source_digest, wait_for

def capture(binary, env, output, page=None, mobile=False, settings=None, player=False, argument=None, width=1180,height=780):
    current=env.copy();current.update(AUVEMIR_SCREENSHOT=str(output),AUVEMIR_SCREENSHOT_WIDTH=str(390 if mobile else width),AUVEMIR_SCREENSHOT_HEIGHT=str(844 if mobile else height))
    if page is not None: current["AUVEMIR_SCREENSHOT_PAGE"]=str(page)
    if mobile: current["AUVEMIR_MOBILE"]="1"
    if settings is not None: current["AUVEMIR_SCREENSHOT_SETTINGS"]=str(settings)
    if player: current["AUVEMIR_SCREENSHOT_PLAYER"]="1"
    result=subprocess.run([str(binary)]+([str(argument)] if argument else []),env=current,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True,timeout=35)
    if result.returncode: raise RuntimeError(result.stdout)
    if not output.is_file(): raise RuntimeError("No native window capture produced: "+result.stdout)
    image=Image.open(output);image.load()
    if image.mode=="RGBA" and image.getextrema()[3]==(255,255): image=image.convert("RGB")
    temporary=output.with_suffix(".optimized.png")
    image.save(temporary,optimize=True);temporary.replace(output)
    assert output.stat().st_size > 100, "Empty image after optimization"
    return result.stdout

def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument("binary",type=Path);parser.add_argument("--output",type=Path,default=ROOT/"docs/screenshots");parser.add_argument("--without-motion",action="store_true");args=parser.parse_args()
    binary=args.binary.resolve();output=args.output.resolve();output.mkdir(parents=True,exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="auvemir-capture-") as directory, display() as session:
        folder=Path(directory);library=create(folder/"media");env=profile_environment(session,folder/"profile")
        capture(binary,env,folder/"import.png",argument=library)
        db=folder/"profile/auvemir.sqlite"
        with sqlite3.connect(db) as conn: assert conn.execute("SELECT count(*) FROM media").fetchone()[0]==12
        captures=[("desktop-home.png",0,False,None,False), ("desktop-library.png",1,False,None,False),("desktop-equalizer.png",11,False,0,False),("settings-appearance.png",11,False,1,False),("phone-home.png",0,True,None,False),("phone-library.png",1,True,None,False),("phone-player.png",1,True,None,True)]
        track=library/"Evening Current/01 - Drift.flac"
        for name,page,mobile,section,player in captures:
            capture(binary,env,output/name,page,mobile,section,player,argument=track if player else None)
        with sqlite3.connect(db) as conn: conn.execute("INSERT INTO settings(key,value) VALUES('appearance-light','true') ON CONFLICT(key) DO UPDATE SET value='true'")
        capture(binary,env,output/"desktop-light.png",page=1)
        if not args.without_motion:
            with sqlite3.connect(db) as conn: conn.execute("UPDATE settings SET value='false' WHERE key='appearance-light'")
            frames=folder/"frames";motion=profile_environment(session,folder/"profile",reduced=False)
            motion.update(AUVEMIR_SCREENSHOT=str(folder/"unused.png"),AUVEMIR_CAPTURE_DIR=str(frames),AUVEMIR_CAPTURE_FRAMES="100",AUVEMIR_CAPTURE_FPS="20",AUVEMIR_SCREENSHOT_DELAY_MS="500")
            with (folder/"motion.log").open("w") as log:
                app=subprocess.Popen([str(binary)],env=motion,stdout=log,stderr=subprocess.STDOUT)
                try:
                    def window_id():
                        if app.poll() is not None: raise RuntimeError((folder/"motion.log").read_text())
                        found=subprocess.run(["xdotool","search","--onlyvisible","--name","^Auvemir$"],env=motion,capture_output=True,text=True)
                        return found.stdout.strip().splitlines()[-1] if found.returncode==0 else None
                    window=wait_for(window_id)
                    input_command(motion,"windowfocus",window)
                    for key in ["ctrl+2","ctrl+comma","ctrl+1"]:
                        time.sleep(1);input_command(motion,"key","--clearmodifiers",key)
                    assert app.wait(timeout=15)==0
                finally:
                    if app.poll() is None: app.kill();app.wait()
            animation=output/"navigation.partial.gif"
            subprocess.run(["ffmpeg","-nostdin","-hide_banner","-loglevel","error","-y","-framerate","20","-i",str(frames/"frame-%04d.png"),"-filter_complex","fps=15,scale=900:-1:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer","-loop","0",str(animation)],check=True)
            with animation.open("rb") as file: os.fsync(file.fileno())
            # Verify all encoded frames before publishing the gallery asset.
            with Image.open(animation) as gif:
                for index in range(gif.n_frames): gif.seek(index);gif.load()
                assert gif.n_frames >= 50
            animation.replace(output/"navigation.gif")
        # Remove owned intermediate encodings before publishing the gallery manifest.
        for leftover in output.glob("*.optimized.png"): leftover.unlink(missing_ok=True)
        for leftover in output.glob("*.partial.gif"): leftover.unlink(missing_ok=True)
        for asset in output.iterdir():
            if asset.suffix in (".png", ".gif"):
                with Image.open(asset) as decoded:
                    for index in range(getattr(decoded, "n_frames", 1)): decoded.seek(index);decoded.load()
        manifest={"source_sha256":source_digest(),"binary_sha256":hashlib.sha256(binary.read_bytes()).hexdigest(),"version":subprocess.check_output([str(binary),"--version"],text=True).strip(),"renderer":"Actual Slint 1.18.1 winit software renderer on Linux/Xvfb", "fixture":"12 generated 45-second quiet sine tones and procedural covers; no commercial media", "limitations":"Phone layout rendered on Linux, not physical-device certification. Playback uses a null ALSA sink, not a physical audio measurement. The GIF is a recording, not an FPS benchmark.","files":{p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(output.iterdir()) if p.suffix in (".png",".gif")}}
        (output/"manifest.json").write_text(json.dumps(manifest,indent=2)+"\n");print(json.dumps(manifest,indent=2))
if __name__=="__main__":main()
