"""Pin libraries; install only one identical benchmark CLI in the reference."""
from pathlib import Path
import hashlib,json,subprocess,sys
HERE=Path(__file__).resolve().parent;ROOT=HERE.parents[2]
reference=Path(sys.argv[1]);out=Path(sys.argv[2]);out.mkdir(parents=True,exist_ok=True)
BASE='d90c80b4001e43807d404454280f1511329ef952';SELECTED='2bb9cf595f675d269406bbe783e88154ed517cb4'
CLI='engine/search/src/bin/lab-phase-speed.rs'
def git(root,*args):return subprocess.check_output(['git','-C',str(root),*args])
assert git(reference,'rev-parse','HEAD').decode().strip()==BASE
assert git(reference,'status','--porcelain').strip()==b''
assert git(ROOT,'rev-parse','HEAD^').decode().strip()==SELECTED
changed=git(ROOT,'diff','--name-only',SELECTED,'HEAD').decode().splitlines()
assert CLI in changed and all(n==CLI or n.startswith(('engine/benchmarks/s26_stack/','.github/workflows/engine-s26l-stack.yml')) for n in changed)
original=git(reference,'show','HEAD:'+CLI)
assert hashlib.sha256(original).hexdigest()=='8cce269c0ceeed232a32fdba37e1b5a3ed7dbaabd97e1a589d5316b0616d19ea'
payload=(ROOT/CLI).read_bytes();(reference/CLI).write_bytes(payload)
assert git(reference,'diff','--name-only').decode().strip()==CLI
(out/'reference-harness.patch').write_bytes(git(reference,'diff','--',CLI))
value=dict(baseline_library=BASE,selected_library=SELECTED,source_sha=git(ROOT,'rev-parse','HEAD').decode().strip(),
    selected_library_unchanged=True,reference_library_unchanged=True,only_reference_change=CLI,
    cli_sha256=hashlib.sha256(payload).hexdigest(),original_cli_sha256=hashlib.sha256(original).hexdigest(),
    source_changes=changed,scope='common transport adds semantic witnesses and finishes result drop before response; library algorithms unchanged')
(out/'provenance.json').write_text(json.dumps(value,indent=2)+'\n')
print(json.dumps(value,indent=2))
