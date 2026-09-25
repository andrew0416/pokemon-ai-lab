"""Check independent-process replay determinism, excluding wall-clock timings."""
import json
import subprocess
import sys
from pathlib import Path
root=Path(__file__).resolve().parents[1]
paths=[]
for _ in range(2):
    completed=subprocess.run([sys.executable,'-X','utf8',str(root/'scripts/lab.py'),
        'battle','--p1','vgc_myopic','--p2','random','--games','2','--seed','99871'],
        cwd=root,capture_output=True,text=True,encoding='utf-8',timeout=180,check=True)
    path=next(line.removeprefix('Artifacts: ') for line in completed.stdout.splitlines() if line.startswith('Artifacts: '))
    paths.append(Path(path))
def normalized(folder):
    result={}
    for file in sorted(folder.glob('*.decisions.json')):
        rows=json.loads(file.read_text(encoding='utf-8'))
        result[file.name]=[{k:v for k,v in row.items() if k!='latency_ms'} for row in rows]
    result['outcomes']=[{k:v for k,v in row.items() if k!='seconds'} for row in json.loads((folder/'outcomes.json').read_text(encoding='utf-8'))]
    return result
assert normalized(paths[0])==normalized(paths[1]), 'Same-seed replay differs across independent processes'
report={'status':'pass','runs':[str(p) for p in paths],'comparison':'all decisions and outcomes, excluding latency and elapsed time'}
(root/'runs/reproducibility.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
print(json.dumps(report,indent=2))
