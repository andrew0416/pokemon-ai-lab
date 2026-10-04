from pathlib import Path
import hashlib,json,re,statistics
R=Path(__file__).resolve().parent;pub=R/'publication';run=37123954718
receipt=json.loads((pub/'receipt.json').read_bytes());folder=pub/f'run-{run}'/f's26g-pipeline-{run}-1'
download=json.loads((folder/'download-receipt.json').read_bytes())
def read(name):
 p=folder/name;assert hashlib.sha256(p.read_bytes()).hexdigest()==download['files'][name],name
 return json.loads(p.read_bytes())
statuses=[json.loads(p.read_bytes()) for p in sorted(pub.glob('status-*.json'))]
status=[s for s in statuses if s.get('id')==run][-1]
assert status['status']=='completed' and status['conclusion']=='success',status
assert status['head_sha']==receipt['source_sha']==(folder/'source-sha.txt').read_text().strip()
tree=read('tree-lp/summary.json');growth=read('growing-lp/summary.json');cadence=read('cadence-lp/summary.json');paper=read('paper-lp/summary.json');pipeline=read('pipeline-lp/summary.json');paired=read('paired/summary.json')
assert tree['passed']==49 and growth['passed']==96 and cadence['passed']==168
assert paper['passed'] and paper['independent_lp'] and paper['certificates']==787
assert pipeline['passed'] and pipeline['independent_lp'] and pipeline['certificates']==652
assert paired['passed'] and paired['timing'] and len(paired['cases'])==30 and len(paired['results'])==36
assert paired['source_sha']==receipt['source_sha']
rows=paired['results'];active=[r for r in rows if not r['censored']]
for r in active:
 assert len(r['pairs'])==9 and r['diagnostics']['converged'] and r['baseline_diagnostics']['converged']
 assert r['diagnostics']['complete'] and r['baseline_diagnostics']['complete']
 assert r['diagnostics']['gap']<=r['tolerance'] and r['baseline_diagnostics']['gap']<=r['tolerance']
 if r['variant'].get('exact_control'):
  for field in ['nodes','information','iterations','value','lower','upper','gap','growth']: assert r['diagnostics'][field]==r['baseline_diagnostics'][field],(r['case'],field)
counts={}
for name in ['core.log','owned.log','on.log','cli.log','off.log']:
 raw=(folder/name).read_bytes();assert hashlib.sha256(raw).hexdigest()==download['files'][name]
 found=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed',raw.decode())
 assert found and all(int(f)==0 for _,f in found);counts[name]=[int(n) for n,_ in found]
summary=[]
for variant in ['frontier-index','owned-compiler','delta-kernel','compressed-checks','all-four','disabled-control']:
 selected=[r for r in active if r['variant']['id']==variant]
 summary.append(dict(variant=variant,cases=len(selected),reductions=[round(r['time_reduction_percent'],4) for r in selected],
   median_descriptive_percent=statistics.median(r['time_reduction_percent'] for r in selected) if selected else None,
   within_runner_faster=sum(r['ratio_bootstrap95'][1]<1 for r in selected),within_runner_slower=sum(r['ratio_bootstrap95'][0]>1 for r in selected),
   interpretation='within-runner paired intervals only; no across-machine guarantee'))
v=dict(schema_version=1,source_sha=receipt['source_sha'],parent_sha=receipt['parent'],run_id=run,run_url=status['html_url'],ci_success=True,
    default_off=True,portable_cpu='x86-64',production_adopted=False,core_tests=counts,independent_lp_certificates=1752,
    new_pipeline_certificates=652,new_fixed_games=29,new_growing_games=16,new_pipeline_converged=pipeline['converged'],new_pipeline_capped=652-pipeline['converged'],
    timing_conditions=30,timing_comparisons=36,eligible_timing_comparisons=len(active),censored_timing_comparisons=36-len(active),
    paired_blocks_per_comparison=9,baseline='S21 LCFR sequence=true periodic32 cadence4; same engine evaluator/budget/pool; step3 baseline is owned-compiler-only',
    elapsed_ci_seconds=None,local_timing=False,win_rate_evaluated=False,summary=summary,results=rows,artifact_directory=folder.as_posix(),
    limitations=['one runner session; bounded six engine fixtures','paired intervals do not represent machine-to-machine variability',
      'sequence coefficient accumulation order can change after ID remapping','original tree certifies only the represented finite game; partial budgets certify the fixed-leaf surrogate',
      'one-turn horizons skip owned and incremental caches because no later snapshot exists','incremental payoff still rebuilds layout/rank/entry vectors and re-sums every coefficient if scale changes',
      'default OFF; no production adoption or win-rate claim'])
from datetime import datetime
v['elapsed_ci_seconds']=(datetime.fromisoformat(status['updated_at'].replace('Z','+00:00'))-datetime.fromisoformat(status['run_started_at'].replace('Z','+00:00'))).total_seconds()
with (R/'RESULTS.json').open('x',encoding='utf-8') as f:json.dump(v,f,ensure_ascii=False,indent=2);f.write('\n')
lines=['# S26g 네 단계 순차 실험 결과','',f'Source `{v["source_sha"]}` · [GitHub Actions]({v["run_url"]})','',
 '1→2→3→4 순서로 구현하고 각 단계의 로컬 정확성 검사를 통과한 다음 단계로 진행했다. 속도는 마지막에 같은 GitHub 러너·같은 범용 x86-64 release 실행파일에서 독립 옵션으로 비교했다. 원본 엔진·기존 탐색 경로는 보존했고 모든 새 옵션은 기본 OFF다.','',
 f'기존 1100건 + 새 652건, 합계 **1752건 독립 LP/전수 BR 인증 검사**가 통과했다. 새 표본은 고정 게임 29개·성장 게임 16개에 여러 옵션·예산을 적용한 652건이다. 새 검사 중 {v["new_pipeline_capped"]}건은 반복 상한에서 미수렴이며, 반환한 인증 수치는 전수 BR/LP와 일치했다. 독립 게임 652개로 해석하지 않는다.','',
 f'속도 조건 30개, 비교 36개 중 {len(active)}개가 공통 gap을 충족했다. 각 비교는 9개 ABBA/BAAB 블록이며, 시간 단축률은 블록별 후보/기준 비율의 중앙값이다. 양수는 빨라짐, 음수는 느려짐이다. 두 시간 중앙값의 단순 비율과는 다를 수 있다.','',
 '| 사례 | 1 인덱스 | 2 저장소 재사용 | 3 보상 캐시 추가 | 4 내부 검사 | 모두 ON |','|---|---:|---:|---:|---:|---:|']
fixtures=list(dict.fromkeys(r['case'].split('-growing-')[0] for r in rows))
for name in fixtures:
 values=[]
 for variant in ['frontier-index','owned-compiler','delta-kernel','compressed-checks','all-four']:
  r=next(r for r in rows if r['case'].split('-growing-')[0]==name and r['variant']['id']==variant)
  values.append('cap' if r['censored'] else f'{r["time_reduction_percent"]:+.2f}%')
 lines.append('| '+name+' | '+' | '.join(values)+' |')
lines+=['','**3번의 기준만 2번 ON 상태다.** 나머지 열은 S21 LCFR+sequence 기준이다. 1턴 국면에서는 1번 선택 루프 및 2/3번 재사용이 실질적으로 작동할 후속 스냅샷이 없으므로 그 차이를 개선 근거로 쓰지 않는다. 각 수치를 합산할 수 없다.','',
 '## 전체 측정 및 구간','', '| 사례 | 옵션 | 기준 ms | 후보 ms | 단축률 | 후보/기준 95% 구간 | 최종 반복 |','|---|---|---:|---:|---:|---|---:|']
for r in rows:
 if r['censored']:lines.append(f'| {r["case"]} | {r["variant"]["id"]} | — | — | cap | — | {r["diagnostics"]["iterations"]} |');continue
 lo,hi=r['ratio_bootstrap95']
 lines.append(f'| {r["case"]} | {r["variant"]["id"]} | {r["baseline_ms"]:.3f} | {r["candidate_ms"]:.3f} | {r["time_reduction_percent"]:+.2f}% | {lo:.4f}–{hi:.4f} | {r["diagnostics"]["iterations"]} |')
lines+=['','## 계약과 제한','',
 '1/2번은 난수 소비·prior 호출·선택 방문·raw tree·정책·반복수·작업 카운터까지 일치시킨다. 3번은 같은 행동열/정보 제약을 유지하지만 합산 순서 때문에 부동소수점 차이가 생길 수 있다. 4번은 중간 검사 필터이며 종료 후보와 마지막 반복은 반드시 원본 Tree 인증을 거친다. 미수렴을 성공으로 표시하지 않는다.','',
 '확장 예산 실패는 컴파일 트리를 넘겨주기 전에 처리한다. 실패한 미공개 builder를 버리고 마지막 해결된 스냅샷을 반환한다. 컴파일·solver 오류는 오류를 반환하며 잘못된 새 정책을 게시하지 않는다.','',
 '3번은 변경 말단과 새 하위 트리만 재방문한다. 레이아웃/순서/entry 벡터 구성은 남아 있으며, scale이 변하면 저장한 원시 말단 기여에서 모든 계수를 재합산한다. 이전 regret의 재사용은 하지 않는다.','',
 f'[구현 명세](<{(R/"source/engine/benchmarks/s26_pipeline/README.md").as_posix()}>) · [기계 조회 결과](<{(R/"RESULTS.json").as_posix()}>)','',
 '작은 차이는 다른 러너 세션에서 재현해야 한다. AVX2는 요구하지 않으며 기존 scalar 경로로 사용 가능하다. 승률은 평가하지 않았다.']
(R/'RESULTS.md').write_text('\n'.join(lines)+'\n',encoding='utf-8')
print(json.dumps({k:v[k] for k in ['source_sha','ci_success','independent_lp_certificates','eligible_timing_comparisons','core_tests','summary']},indent=2))
