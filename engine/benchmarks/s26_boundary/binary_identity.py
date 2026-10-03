"""Fail closed when distinct version builds accidentally reuse one executable."""
from pathlib import Path
import hashlib,json,sys
root=Path(sys.argv[1]);out=Path(sys.argv[2])
names=('s24-speed','s25-speed','speed','s25-memory','memory')
hashes={name:hashlib.sha256((root/name).read_bytes()).hexdigest() for name in names}
assert len({hashes[name] for name in names[:3]})==3,('speed builds unexpectedly identical',hashes)
assert hashes['s25-memory']!=hashes['memory'],('memory builds unexpectedly identical',hashes)
out.write_text(json.dumps(dict(passed=True,binary_sha256=hashes,scope='distinct artifacts guard; source pins and per-sample witnesses separately verify provenance'),indent=2)+'\n')
print(json.dumps(dict(passed=True,distinct_speed_binaries=3,distinct_memory_binaries=2)))
