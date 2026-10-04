from pathlib import Path
import json
R=Path(__file__).resolve().parent;r=json.loads((R/'RESULTS.json').read_bytes())
decisions=[]
for variant in ['frontier-index','owned-compiler','delta-kernel','compressed-checks','all-four']:
 rows=[v for v in r['results'] if v['variant']['id']==variant and not v['censored']]
 if variant in ['frontier-index','owned-compiler','delta-kernel']:
  meaningful=[v for v in rows if v['baseline_diagnostics']['growth']['solves']>1]
 else:meaningful=rows
 faster=[v['case'] for v in meaningful if v['ratio_bootstrap95'][1]<1]
 slower=[v['case'] for v in meaningful if v['ratio_bootstrap95'][0]>1]
 decisions.append(dict(variant=variant,meaningful_cases=len(meaningful),within_runner_faster=faster,within_runner_slower=slower,
  disposition='conditional-replication-candidate' if faster and not slower else 'mixed-keep-off' if faster else 'no-measured-benefit-keep-off',
  production_adopted=False,default_on=False,scope='bounded single-runner experiment; no cross-run confidence or win-rate inference'))
v=dict(source_sha=r['source_sha'],directions_implemented=4,independent_and_cumulative_tested=True,production_adopted=False,default_off=True,
 decisions=decisions,remaining='Only follow-up candidate replication/selection is proposed; requested four implementations and their bounded evaluation are complete.')
with (R/'DECISION.json').open('x',encoding='utf-8') as f:json.dump(v,f,indent=2);f.write('\n')
print(json.dumps(v,indent=2))
