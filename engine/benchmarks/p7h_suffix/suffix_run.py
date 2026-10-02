"""Single CLI for shared build, fresh accuracy gate, then paired timing."""
from pathlib import Path
import argparse
import suffix_build as build
import suffix_accuracy as accuracy
import suffix_gate as gate
import suffix_timing as timing
def main():
    ap=argparse.ArgumentParser();ap.add_argument('stage',choices=('build','full500','broad','accuracy_gate','timing','aggregate'))
    ap.add_argument('--workspace',type=Path,required=True);ap.add_argument('--shard',type=int);ap.add_argument('--package-sha');ap.add_argument('--gate-sha')
    args=ap.parse_args();workspace=args.workspace.resolve()
    if args.stage=='build':build.build(workspace);return
    if args.stage=='full500':code=accuracy.full500(workspace,args.shard,args.package_sha)
    elif args.stage=='broad':code=accuracy.broad(workspace,args.package_sha)
    elif args.stage=='accuracy_gate':code=gate.issue(workspace,args.package_sha)
    elif args.stage=='timing':code=timing.timing(workspace,args.shard,args.package_sha,args.gate_sha)
    else:code=timing.aggregate(workspace,args.package_sha,args.gate_sha)
    raise SystemExit(code)
if __name__=='__main__':main()
