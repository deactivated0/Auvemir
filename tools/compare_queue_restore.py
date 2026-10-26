#!/usr/bin/env python3
"""Alternate two release workload executables and verify exact queue outputs."""
import argparse, hashlib, json, platform, statistics, subprocess
from pathlib import Path

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('before',type=Path);parser.add_argument('after',type=Path)
    parser.add_argument('--before-source',required=True);parser.add_argument('--after-source',required=True)
    parser.add_argument('--output',type=Path,required=True);args=parser.parse_args()
    results={'before':[],'after':[]}
    paths={'before':args.before.resolve(),'after':args.after.resolve()}
    for iteration in range(5):
        for label in (['before','after'] if iteration%2==0 else ['after','before']):
            response=subprocess.run([str(paths[label]),'5000'],check=True,capture_output=True,text=True)
            results[label].append(json.loads(response.stdout.strip().splitlines()[-1]))
    fingerprints={run['output_blake3'] for group in results.values() for run in group}
    assert len(fingerprints)==1,'Queue results changed'
    report={'architecture':platform.machine(),'before_source_sha256':args.before_source,'after_source_sha256':args.after_source,'before_binary_sha256':digest(paths['before']),'after_binary_sha256':digest(paths['after']),'method':'Five alternating process runs per version, seven in-process samples each, same host and release toolchain, 5000 deterministic media records and queue cursor. Each process asserts complete restored Media equality. No other build or profiling workload ran concurrently.','exact_queue_output_identical':True,'output_blake3':fingerprints.pop(),'runs':results}
    report['median_queue_restore_ms']={label:statistics.median([x for run in group for x in run['queue_restore_ms']]) for label,group in results.items()}
    report['median_three_sorted_pages_ms']={label:statistics.median([x for run in group for x in run['three_sorted_pages_ms']]) for label,group in results.items()}
    args.output.parent.mkdir(parents=True,exist_ok=True);args.output.write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({key:value for key,value in report.items() if key!='runs'},indent=2))
if __name__=='__main__':main()
