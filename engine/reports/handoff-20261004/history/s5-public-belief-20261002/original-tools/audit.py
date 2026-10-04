"""Re-evaluate fetched policies as mixtures of pure contingent plans, without Rust/SciPy."""
from pathlib import Path
import hashlib,itertools,json,math,re
import publish
ROOT=Path(__file__).resolve().parent;RUN=37014537106
P=ROOT/f'publication/run-{RUN}/s26b-correctness-{RUN}-1'
read=lambda p:json.loads(p.read_bytes());sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
receipt=read(P/'download-receipt.json');publication=read(ROOT/'publication/receipt.json')
for name,digest in receipt['files'].items():assert sha(P/name)==digest
for name,digest in publication['changed_sha256'].items():assert sha(ROOT/'source'/name)==digest
assert (P/'source-sha.txt').read_text().strip()==publication['source_sha']
assert publish.snapshot()==publication['shared_git']
counts={name:sum(map(int,re.findall(r'test result: ok\. (\d+) passed;', (P/name).read_text()))) for name in ['on.log','cli.log','off.log']}
assert counts=={'on.log':68,'cli.log':2,'off.log':17}
summary=read(P/'tree-lp/summary.json');assert summary['passed']==49 and summary['converged']==49
for case in summary['cases']:
 name=case['case'];request=read(P/f'tree-lp/{name}-request.json');result=read(P/f'tree-lp/{name}-result.json')
 info=[{},{}]
 for node in request['nodes']:
  if node['type']=='decision':info[node['player']][node['information']]=node['actions']
 keys=[list(v) for v in info];plans=[list(itertools.product(*[range(len(m)) for m in v.values()])) for v in info]
 policy={p['key']:p for p in result['policies']}
 assert set(policy)==set(keys[0]+keys[1])
 for side in range(2):
  for key,actions in info[side].items():assert policy[key]['actions']==actions and policy[key]['player']==side
 for p in policy.values():assert min(p['probabilities'])>=0 and abs(sum(p['probabilities'])-1)<1e-10
 mixed=[[math.prod(policy[key]['probabilities'][a] for key,a in zip(keys[side],plan)) for plan in plans[side]] for side in range(2)]
 assert all(abs(sum(v)-1)<1e-10 for v in mixed)
 def evaluate(id,chosen):
  node=request['nodes'][id]
  if node['type']=='terminal':return node['value']
  if node['type']=='chance':return sum(e['probability']*evaluate(e['child'],chosen) for e in node['edges'])
  return evaluate(node['children'][chosen[node['player']][node['information']]],chosen)
 A=[[evaluate(request['root'],[dict(zip(keys[0],p)),dict(zip(keys[1],q))]) for q in plans[1]] for p in plans[0]]
 upper=max(sum(v*q for v,q in zip(row,mixed[1])) for row in A)
 lower=min(sum(mixed[0][r]*A[r][c] for r in range(len(A))) for c in range(len(A[0])))
 value=sum(mixed[0][r]*sum(v*q for v,q in zip(A[r],mixed[1])) for r in range(len(A)))
 gap=upper-lower
 for key,expected in [('value',value),('lower',lower),('upper',upper),('finite_game_gap',gap)]:assert abs(result[key]-expected)<1e-8,(name,key)
 assert lower-1e-7<=case['lp_value']<=upper+1e-7 and gap<=request['solver']['tolerance']+1e-8
 if name=='kuhn':assert abs(case['lp_value']+1/18)<1e-12
matrix=read(P/'matrix-lp/summary.json');assert matrix['passed']==72 and matrix['converged']==72
for case in matrix['cases']:
 i=case['case'];q=read(P/f'matrix-lp/{i:02d}-request.json');r=read(P/f'matrix-lp/{i:02d}-result.json');p=[x['probability'] for x in r['ours']]
 likes={x['world']:x['probability'] for x in q.get('likelihoods',[])};weights=[w['weight']*likes.get(w['id'],1.) for w in q['worlds']];total=sum(weights);weights=[w/total for w in weights]
 lower=0.;upper_rows=[0.]*len(p);profile=0.
 for w,s,b in zip(q['worlds'],r['worlds'],weights):
  assert w['id']==s['id'] and abs(b-s['posterior'])<1e-12
  cols=[x['probability'] for x in s['theirs']];m=len(cols);A=w['payoffs']
  rv=[sum(cols[c]*A[j*m+c] for c in range(m)) for j in range(len(p))]
  lower+=b*min(sum(p[j]*A[j*m+c] for j in range(len(p))) for c in range(m));upper_rows=[x+b*v for x,v in zip(upper_rows,rv)];profile+=b*sum(a*v for a,v in zip(p,rv))
 upper=max(upper_rows)
 for key,v in [('lower',lower),('upper',upper),('value',profile),('fixed_matrix_gap',upper-lower)]:assert abs(r[key]-v)<1e-9
 assert lower-1e-7<=case['lp_value']<=upper+1e-7
local=read(ROOT/'validation/r3/receipt.json');smoke=read(ROOT/'validation/engine-cli/receipt.json');assert local['passed'] and smoke['passed']
result=dict(source_sha=publication['source_sha'],run=RUN,conclusion='success',run_wall_seconds=82,rust_tests=sum(counts.values()),test_groups=counts,
 new_information_tests=15,extensive_lp_cases=49,extensive_converged=49,max_extensive_gap=summary['max_gap'],matrix_lp_regression_cases=72,
 matrix_converged=72,max_matrix_gap=matrix['max_gap'],raw_best_response_audits=121,kuhn_lp_value=-1/18,
 engine_cli_cases=smoke['cases'],no_default_features_passed=True,clippy_passed=True,default_off=True,production_changed=False,
 full_battle_log_information=False,performance_measured=False,artifact_sha256=receipt['archive_sha256'],shared_git=publication['shared_git'])
with (ROOT/f'AUDIT-{RUN}.json').open('x',encoding='utf-8') as f:json.dump(result,f,indent=2);f.write('\n')
print(json.dumps({k:v for k,v in result.items() if k!='engine_cli_cases'},indent=2))
