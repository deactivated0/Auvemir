"""Isolated authenticated X server and app data for development tests only."""
import contextlib, hashlib, os, secrets, socket, subprocess, tempfile, time
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]

def wait_for(predicate, timeout=8):
    deadline=time.monotonic()+timeout
    while time.monotonic()<deadline:
        value=predicate()
        if value: return value
        time.sleep(.05)
    raise AssertionError("Timed out waiting for native application state")

@contextlib.contextmanager
def display():
    with tempfile.TemporaryDirectory(prefix="auvemir-x11-") as directory:
        auth=Path(directory)/"Xauthority";auth.touch()
        number=97;address=f"127.0.0.1:{number}"
        subprocess.run(["xauth","-f",str(auth),"add",address,".",secrets.token_hex(16)],check=True,stdout=subprocess.DEVNULL)
        server=subprocess.Popen(["Xvfb",f":{number}","-screen","0","2560x1800x24","-auth",str(auth),"-listen","tcp","-nolisten","unix"],stdout=subprocess.DEVNULL,stderr=subprocess.PIPE)
        try:
            def ready():
                if server.poll() is not None: raise RuntimeError(server.stderr.read().decode())
                try:
                    with socket.create_connection(("127.0.0.1",6000+number),timeout=.2): return True
                except OSError: return False
            wait_for(ready)
            env=os.environ.copy();env.update(DISPLAY=address,XAUTHORITY=str(auth),SLINT_BACKEND="winit-software",SLINT_SCALE_FACTOR="1")
            yield env
        finally:
            server.terminate()
            try: server.wait(timeout=3)
            except subprocess.TimeoutExpired: server.kill();server.wait()

def profile_environment(environment, profile, reduced=True):
    profile=Path(profile);profile.mkdir(parents=True,exist_ok=True)
    alsa=profile/"alsa.conf";alsa.write_text("pcm.!default { type null }\n")
    env=environment.copy();env.update(AUVEMIR_DATA_DIR=str(profile),ALSA_CONFIG_PATH=str(alsa),AUVEMIR_REDUCE_MOTION="1" if reduced else "0",NO_PROXY="",no_proxy="")
    # Demo captures never contact real catalogs, accounts or radios.
    for name in ["HTTP_PROXY","HTTPS_PROXY","ALL_PROXY","http_proxy","https_proxy","all_proxy"]: env[name]="http://127.0.0.1:1"
    return env

def input_command(env,*args):
    return subprocess.check_output(["xdotool",*map(str,args)],env=env,text=True).strip()

def source_digest():
    digest=hashlib.sha256()
    paths=[ROOT/"Cargo.toml",ROOT/"Cargo.lock"]
    for folder in [ROOT/"crates",ROOT/"ui"]: paths.extend(p for p in folder.rglob("*") if p.is_file())
    for path in sorted(paths): digest.update(str(path.relative_to(ROOT)).encode());digest.update(b"\0");digest.update(path.read_bytes())
    return digest.hexdigest()
