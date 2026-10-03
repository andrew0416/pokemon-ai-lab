"""Only the common benchmark CLI is patched in frozen reference checkouts."""
from pathlib import Path
import hashlib,json,subprocess,sys
HERE=Path(__file__).resolve().parent
pins={'s24':'d90c80b4001e43807d404454280f1511329ef952','s25':'2bb9cf595f675d269406bbe783e88154ed517cb4'}
root=Path(sys.argv[1]);label=sys.argv[2];out=Path(sys.argv[3]);out.mkdir(parents=True,exist_ok=True)
def git(*args):return subprocess.check_output(['git','-C',str(root),*args]).decode().strip()
assert git('rev-parse','HEAD')==pins[label] and git('status','--porcelain')==''
name='engine/search/src/bin/lab-phase-speed.rs';p=root/name
original_blob=subprocess.check_output(['git','-C',str(root),'show','HEAD:'+name])
assert hashlib.sha256(original_blob).hexdigest()=='8cce269c0ceeed232a32fdba37e1b5a3ed7dbaabd97e1a589d5316b0616d19ea'
assert p.read_bytes().replace(b'\r\n',b'\n')==original_blob.replace(b'\r\n',b'\n')
source=HERE.parents[2]/name;payload=source.read_bytes();p.write_bytes(payload)
assert git('diff','--name-only')==name
(out/(label+'-harness.patch')).write_text(git('diff','--',name),encoding='utf-8')
(out/(label+'-reference.json')).write_text(json.dumps(dict(source_sha=pins[label],original_cli_sha256='8cce269c0ceeed232a32fdba37e1b5a3ed7dbaabd97e1a589d5316b0616d19ea',patched_cli_sha256=hashlib.sha256(payload).hexdigest(),only_changed=name,library_unchanged=True),indent=2)+'\n')
print(label+' reference library frozen; common benchmark transport installed')
