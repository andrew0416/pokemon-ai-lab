from pathlib import Path
import hashlib,json,math,re
ROOT=Path(__file__).resolve().parent
read=lambda p:json.loads(p.read_bytes())
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
package=ROOT/'publication/run-37008543602/s26-correctness-37008543602-1'
receipt=read(package/'download-receipt.json')
for name,digest in receipt['files'].items():assert sha(package/name)==digest
publication=read(ROOT/'publication/receipt.json')
assert (package/'source-sha.txt').read_text().strip()==publication['source_sha']
for name,digest in publication['changed_sha256'].items():assert sha(ROOT/'source'/name)==digest
summary=read(package/'lp/summary.json');assert summary['passed']==72 and len(summary['cases'])==72
counts={name:int(re.search(r'test result: ok\. (\d+) passed;', (package/name).read_text()).group(1)) for name in ['on.log','cli.log','off.log']}
assert counts=={'on.log':53,'cli.log':1,'off.log':17}
for case in summary['cases']:
 i=case['case'];request=read(package/f'lp/{i:02d}-request.json');result=read(package/f'lp/{i:02d}-result.json')
 n=len(request['rows']);ours=[v['probability'] for v in result['ours']]
 assert [v['action'] for v in result['ours']]==request['rows'] and abs(sum(ours)-1)<1e-10
 lookup={v['world']:v['probability'] for v in request.get('likelihoods',[])}
 weights=[w['weight']*lookup.get(w['id'],1.) for w in request['worlds']];total=sum(weights);weights=[w/total for w in weights]
 lower=0.;profile=0.;rows=[0.]*n
 for w,report,b in zip(request['worlds'],result['worlds'],weights):
  assert w['id']==report['id'] and abs(b-report['posterior'])<1e-12
  q=[v['probability'] for v in report['theirs']];m=len(q);A=w['payoffs']
  assert abs(sum(q)-1)<1e-10 and min(q)>=0
  rv=[sum(q[c]*A[r*m+c] for c in range(m)) for r in range(n)]
  world_value=sum(p*v for p,v in zip(ours,rv));assert abs(world_value-report['value'])<1e-10
  lower+=b*min(sum(ours[r]*A[r*m+c] for r in range(n)) for c in range(m))
  profile+=b*world_value;rows=[x+b*y for x,y in zip(rows,rv)]
 upper=max(rows);gap=upper-lower
 for key,value in [('value',profile),('lower',lower),('upper',upper),('fixed_matrix_gap',gap)]:assert abs(result[key]-value)<1e-9
 assert lower-1e-7<=case['lp_value']<=upper+1e-7
 assert result['converged']==(result['fixed_matrix_gap']<=request['solver']['tolerance'])
 assert abs(gap-case['gap'])<1e-9 and gap<=.05
local=read(ROOT/'validation/r1/receipt.json');assert local['passed']
smoke=read(ROOT/'validation/cli-engine/receipt.json');assert smoke['passed']
result=dict(source_sha=publication['source_sha'],run=37008543602,conclusion='success',run_wall_seconds=77,
 rust_tests=sum(counts.values()),test_groups=counts,expanded_normal_form_cases=24,independent_lp_cases=72,
 lp_converged=summary['converged'],max_lp_gap=summary['max_gap'],raw_certificate_audits=72,
 engine_cli_rows=smoke['rows'],engine_cli_world_columns=smoke['columns'],engine_cli_gap=smoke['gap'],
 no_default_features_passed=True,clippy_passed=True,default_off=True,production_changed=False,performance_measured=False,
 artifact_sha256=receipt['archive_sha256'],shared_git=publication['shared_git'])
with (ROOT/'AUDIT-37008543602.json').open('x',encoding='utf-8') as f:json.dump(result,f,indent=2);f.write('\n')
print(json.dumps(result,indent=2))
