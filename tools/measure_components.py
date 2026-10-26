#!/usr/bin/env python3
"""Record raw component timings and exact output checks for the two final passes."""
import argparse, json, platform, subprocess
from pathlib import Path
from native_session import ROOT, source_digest

def run(command):
    result=subprocess.run(command,cwd=ROOT,text=True,capture_output=True,check=True)
    return result.stdout

def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument("--output",type=Path,required=True);args=parser.parse_args()
    queue=json.loads(run(["cargo","run","--locked","--release","-p","auvemir-core","--example","ui_workload","--","5000"]).strip().splitlines()[-1])
    text=run(["cargo","test","--locked","--release","-p","auvemir","--lib","artwork::tests::thumbnail_handoff_benchmark","--","--ignored","--exact","--nocapture"])
    line=next(line for line in text.splitlines() if line.startswith("ARTWORK_BENCHMARK "))
    result={"source_sha256":source_digest(),"architecture":platform.machine(),"queue":queue,"artwork":json.loads(line.removeprefix("ARTWORK_BENCHMARK ")),"method":"Seven in-process samples per workload in release mode. 5000 queue items, 5000 handoffs of a decoded 192x192 RGBA thumbnail. Exact restored media/cursor and image pixels are asserted. Component latency is not whole-app or hardware FPS."}
    args.output.parent.mkdir(parents=True,exist_ok=True);args.output.write_text(json.dumps(result,indent=2)+"\n");print(json.dumps(result,indent=2))
if __name__=="__main__":main()
