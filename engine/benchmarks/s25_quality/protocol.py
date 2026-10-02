"""Read-only scoring of completed root policies on a common full-action reference."""
import hashlib
import json
import math
from pathlib import Path

HERE = Path(__file__).resolve().parent
PLAN = json.loads((HERE/'plan.json').read_bytes())

def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()

def write(path, value):
    with Path(path).open('x', encoding='utf-8', newline='\n') as f:
        json.dump(value, f, ensure_ascii=False, indent=2, allow_nan=False)
        f.write('\n')

def records(path):
    raw=Path(path).read_bytes()
    lines=raw.splitlines(keepends=True)
    result=[]
    for i,line in enumerate(lines):
        if not line.endswith(b'\n') and i == len(lines)-1:
            # Owned process may be killed mid-write. Keep raw bytes, never use partial policy.
            continue
        result.append(json.loads(line))
    return result

def policy(vector, n):
    assert len(vector)==n and all(math.isfinite(x) and x>=0 for x in vector), 'invalid policy'
    total=sum(vector)
    assert abs(total-1)<1e-5, 'invalid probability mass'
    return [x/total for x in vector]

def select(rows, limit, key='elapsed_s'):
    candidates=[r for r in rows if r.get('kind')=='policy' and r[key]<=limit]
    # A policy that finishes after a deadline is NEVER backdated into that deadline.
    return candidates[-1] if candidates else None

def quality(snapshot, ref):
    if snapshot is None or ref is None:
        return None
    n,m=ref['rows'],ref['cols']
    p,q=policy(snapshot['rows'],n),policy(snapshot['cols'],m)
    rp,rq=policy(ref['row_policy'],n),policy(ref['col_policy'],m)
    a=ref['matrix'];lo=ref['cell_lower'];hi=ref['cell_upper']
    assert len(a)==len(lo)==len(hi)==n*m
    assert all(math.isfinite(x) for v in (a,lo,hi) for x in v)
    row_response=lambda cells:max(sum(cells[r*m+c]*q[c] for c in range(m)) for r in range(n))
    col_response=lambda cells:min(sum(p[r]*cells[r*m+c] for r in range(n)) for c in range(m))
    value=sum(p[r]*a[r*m+c]*q[c] for r in range(n) for c in range(m))
    mode=lambda v:max(range(len(v)),key=v.__getitem__)
    return dict(reference_root_br_gap=row_response(a)-col_response(a),
        reference_gap_lower=max(0.,row_response(lo)-col_response(hi)),
        reference_gap_upper=row_response(hi)-col_response(lo),
        policy_value_on_reference=value,estimated_value_error=abs(snapshot['estimated_value']-ref['value']),
        row_total_variation=sum(abs(x-y) for x,y in zip(p,rp))/2,
        col_total_variation=sum(abs(x-y) for x,y in zip(q,rq))/2,
        modal_row_agrees=mode(p)==mode(rp),modal_col_agrees=mode(q)==mode(rq),
        reference_value_interval=[ref['lower'],ref['upper']],
        reference_max_cell_interval=max(h-l for l,h in zip(lo,hi)))

def score_trial(raw, reference, seconds, work):
    errors=[r for r in raw if r.get('kind')=='error']
    ready=[r for r in raw if r.get('kind')=='ready']
    assert len(ready)==1, 'missing/duplicate ready'
    snapshots=[r for r in raw if r.get('kind')=='policy']
    for a,b in zip(snapshots,snapshots[1:]):
        assert a['elapsed_s']<=b['elapsed_s'] and a['transitions']<=b['transitions']
    for snapshot in snapshots:
        policy(snapshot['rows'],len(ready[0]['actions'][0]))
        policy(snapshot['cols'],len(ready[0]['actions'][1]))
        assert math.isfinite(snapshot['estimated_value']) and math.isfinite(snapshot['local_gap'])
    # A later domain failure invalidates this run, instead of silently dropping bad pairs.
    valid=not errors
    def row(limit,key):
        p=select(raw,limit,key) if valid else None
        return dict(limit=limit,available=p is not None,policy=p,quality=quality(p,reference))
    return dict(valid=valid,errors=errors,ready=ready[0],
        time=[row(t,'elapsed_s') for t in seconds],
        work=[row(t,'transitions') for t in work])
