#!/usr/bin/env python3
"""Launch the real simulator app and run the shared decoder fixture checks."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import time
ROOT=Path(__file__).resolve().parents[1]
def run(args,**options):return subprocess.run([str(a) for a in args],check=True,**options)
def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('app',type=Path);args=parser.parse_args()
    devices=json.loads(subprocess.check_output(['xcrun','simctl','list','devices','available','-j']))['devices']
    candidates=[d for runtime,items in devices.items() if '.iOS-' in runtime for d in items if d.get('isAvailable') and 'iPhone' in d['name']]
    if not candidates:raise RuntimeError('The runner has no available iPhone simulator runtime')
    selected=candidates[0];device=selected['udid'];report={'simulator':selected['name'],'physical_device':False,'launch':False,'native_codecs':False}
    try:
        if selected['state']!='Booted':run(['xcrun','simctl','boot',device])
        run(['xcrun','simctl','bootstatus',device,'-b'],timeout=180)
        run(['xcrun','simctl','install',device,args.app])
        run(['xcrun','simctl','launch',device,'org.auvemir.app']);time.sleep(5)
        listing=subprocess.check_output(['xcrun','simctl','spawn',device,'launchctl','list'],text=True)
        if not any('org.auvemir.app' in line and line.split()[0].isdigit() for line in listing.splitlines()):raise RuntimeError('The simulator application did not remain running')
        report['launch']=True
        run(['xcrun','simctl','io',device,'screenshot',ROOT/'docs/ios-simulator.png'])
        container=subprocess.check_output(['xcrun','simctl','get_app_container',device,'org.auvemir.app','data'],text=True).strip()
        wrapper=ROOT/'target/ios-sim-decoder';wrapper.parent.mkdir(exist_ok=True)
        wrapper.write_text((ROOT/'tests/ios_sim_decoder.py').read_text());wrapper.chmod(0o700)
        env={**os.environ,'AUVEMIR_SIMULATOR_ID':device,'AUVEMIR_SIMULATOR_DATA':container}
        run(['python3',ROOT/'tests/verify_codecs.py',wrapper,'--report',ROOT/'docs/ios-codec-results.json'],env=env)
        report['native_codecs']=True
    finally:
        (ROOT/'docs/ios-simulator-results.json').write_text(json.dumps(report,indent=2)+'\n')
        subprocess.run(['xcrun','simctl','terminate',device,'org.auvemir.app'],check=False,capture_output=True)
        subprocess.run(['xcrun','simctl','shutdown',device],check=False,capture_output=True)
if __name__=='__main__':main()
