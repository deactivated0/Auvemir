#!/usr/bin/env python3
"""Measure a fixed native startup/idle/render window, not pure startup or hardware FPS."""
import argparse, hashlib, json, os, shutil, sqlite3, subprocess, tempfile, time
from pathlib import Path
from capture_screenshots import capture
from create_demo_library import create
from native_session import display, profile_environment, source_digest

def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument("binary",type=Path);parser.add_argument("--output",type=Path,required=True);args=parser.parse_args();binary=args.binary.resolve();samples=[]
    with tempfile.TemporaryDirectory(prefix="auvemir-profile-") as directory, display() as session:
        folder=Path(directory);library=create(folder/"media");env=profile_environment(session,folder/"seed");capture(binary,env,folder/"seed.png",argument=library)
        database=folder/"seed/auvemir.sqlite"
        with sqlite3.connect(database) as conn:
            row=conn.execute("SELECT kind,title,artist,album,location,duration_ms,artwork,favorite,provider,group_id,track_number FROM media ORDER BY title LIMIT 1").fetchone()
            conn.execute("DELETE FROM media")
            ids=[]
            for i in range(5000):
                identity=f"fixture-{i:06}";ids.append(identity);values=list(row);values[1]=f"Track {i:06}";values[2]=f"Artist {i%50:02}";values[3]=f"Album {i%200:03}"
                conn.execute("INSERT INTO media(id,kind,title,artist,album,location,duration_ms,artwork,favorite,provider,group_id,track_number) VALUES(?,?,?,?,?,?,?,?,?,?,?,?)",(identity,*values))
            conn.execute("INSERT INTO settings(key,value) VALUES('queue',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",(json.dumps([ids,2500]),))
        for i in range(3):
            profile=folder/f"run-{i}";shutil.copytree(folder/"seed",profile);current=profile_environment(session,profile)
            output=folder/f"capture-{i}.png";current.update(AUVEMIR_SCREENSHOT=str(output),AUVEMIR_SCREENSHOT_PAGE="1",AUVEMIR_SCREENSHOT_DELAY_MS="1000")
            with (folder/f"run-{i}.log").open("w") as log:
                started=time.monotonic();process=subprocess.Popen([str(binary)],env=current,stdout=log,stderr=subprocess.STDOUT)
                _,status,usage=os.wait4(process.pid,0);process.returncode=os.waitstatus_to_exitcode(status)
                assert process.returncode==0 and output.stat().st_size>100
                samples.append({"wall_ms":(time.monotonic()-started)*1000,"cpu_ms":(usage.ru_utime+usage.ru_stime)*1000,"peak_rss_kib":usage.ru_maxrss,"capture_sha256":hashlib.sha256(output.read_bytes()).hexdigest()})
        report={"source_sha256":source_digest(),"binary_bytes":binary.stat().st_size,"queue_tracks":5000,"samples":samples,"method":"Three isolated runs, warm OS cache, same generated metadata and artwork, restored 5000-track queue, native software rendering, 1000 ms capture timer. Wall time includes the fixed timer and PNG encoding. CPU covers startup/idle/render together, not pure idle. RSS is wait4 child peak. No audio hardware or FPS measurement."}
        args.output.parent.mkdir(parents=True,exist_ok=True);args.output.write_text(json.dumps(report,indent=2)+"\n");print(json.dumps(report,indent=2))
if __name__=="__main__":main()
