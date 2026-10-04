from pathlib import Path
import hashlib,json,re
R=Path(__file__).resolve().parent;pub=R/'publication';run=37119405706
receipt=json.loads((pub/'receipt.json').read_bytes());folder=pub/f'run-{run}'/f's26f-papers-{run}-1'
download=json.loads((folder/'download-receipt.json').read_bytes())
def read(name):
    p=folder/name;assert hashlib.sha256(p.read_bytes()).hexdigest()==download['files'][name],name
    return json.loads(p.read_bytes())
statuses=[json.loads(p.read_bytes()) for p in sorted(pub.glob('status-*.json'))]
status=[s for s in statuses if s.get('id')==run][-1]
assert status['status']=='completed' and status['conclusion']=='success'
assert status['head_sha']==receipt['source_sha']==(folder/'source-sha.txt').read_text().strip()
paper=read('paper-lp/summary.json');tree=read('tree-lp/summary.json');growth=read('growing-lp/summary.json');cadence=read('cadence-lp/summary.json');paired=read('paired/summary.json')
assert paper['passed'] and paper['independent_lp'] and paper['certificates']==787
assert tree['passed']==49 and growth['passed']==96 and cadence['passed']==168
assert paired['passed'] and paired['timing'] and len(paired['cases'])==18 and len(paired['results'])==150
assert paired['source_sha']==receipt['source_sha']
rows=paired['results'];active=[r for r in rows if not r['censored']]
for r in active:
    assert len(r['pairs'])==7 and r['diagnostics']['converged'] and r['baseline_diagnostics']['converged']
    assert r['diagnostics']['complete'] and r['baseline_diagnostics']['complete']
    assert r['diagnostics']['gap']<=r['tolerance'] and r['baseline_diagnostics']['gap']<=r['tolerance']
def compact(r):
    out={k:r[k] for k in ['case','phase','tolerance','censored','variant','diagnostics','baseline_diagnostics']}
    for k in ['baseline_ms','candidate_ms','time_reduction_percent','speedup','ratio_bootstrap95']: 
        if k in r:out[k]=r[k]
    return out
best=[]
for c in paired['cases']:
    selected=[r for r in active if r['case']==c['case'] and r['variant']['id']!='lcfr-control']
    if selected:best.append(compact(min(selected,key=lambda r:r['ratio_median'])))
test_counts={}
for name in ['core.log','owned.log','on.log','cli.log','off.log']:
    text=(folder/name).read_text();counts=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed',text)
    assert counts and all(int(f)==0 for _,f in counts)
    test_counts[name]=[int(n) for n,_ in counts]
v=dict(schema_version=1,source_sha=receipt['source_sha'],parent_sha=receipt['parent'],run_id=run,run_url=status['html_url'],
    ci_success=True,default_off=True,portable_cpu='x86-64',production_adopted=False,production_git_unchanged=True,
    core_tests=test_counts,independent_lp_certificates=1100,new_paper_certificates=787,new_fixed_games=29,new_growing_games=12,
    new_paper_converged=paper['converged'],new_paper_capped=787-paper['converged'],new_paper_warm_applications=paper['warm_applications'],
    timing_cases=18,timing_pairs=150,eligible_timing_pairs=len(active),censored_timing_pairs=150-len(active),
    warm_baseline='plain simultaneous CFR cold; not S19 LCFR',growth_baseline='original S19 LCFR periodic32 cadence4; pivot serial/others4threads',
    fixed_baseline='LCFR geometric; fixed engine tree outside timer; kernel initialization and original certificate inside timer',
    seven_paired_blocks=True,local_timing=False,win_rate_evaluated=False,
    best_by_case=best,results=[compact(r) for r in rows],artifact_directory=folder.as_posix(),
    limitations=['single runner and bounded fixture corpus','geometric checks can overshoot the requested threshold','different algorithms may return different equilibrium policies','partial growth trajectories can differ; only complete same games timed','kernel compression changes floating-point summation order','no production adoption and no strength claim'])
with (R/'RESULTS.json').open('x',encoding='utf-8') as f:json.dump(v,f,ensure_ascii=False,indent=2);f.write('\n')
lines=['# S26f 1~3 구현·검증·속도 결과','',f'Source `{v["source_sha"]}` · [GitHub Actions]({v["run_url"]})','',
 'CFR 규칙 교체, 정책 재인증 및 warm start, 행렬/다중 턴 행동열 압축을 각각 독립 옵션으로 구현했다. 실험 전용이며 기본 OFF다. 원래 scalar/serial 경로, 범용 x86-64 빌드, 생산 코드와 공유 Git 체크아웃을 보존했다.','',
 f'독립 LP 인증은 기존 313건과 새 {v["new_paper_certificates"]}건을 합쳐 1100건 통과했다. 새 표본은 고정 게임 29개·성장 게임 12개에 여러 규칙/예산을 적용한 것이며, 787개의 독립 게임은 아니다. 새 검사의 {v["new_paper_capped"]}건은 반복 cap에서 미수렴으로 정직하게 표시됐다. 인증 수치 자체는 전수 best response/LP와 일치했다.','',
 f'18개 실측 조건에서 150개 pair 중 {len(active)}개가 같은 gap 조건을 충족했다. 나머지는 cap 비수렴으로 단축률 집계에서 제외했다. 7개 ABBA/BAAB block의 중앙 비율과 paired bootstrap 95% 구간이다. 기계 간 불확실성이나 승률을 뜻하지 않는다.','',
 '## 전체 성장 시간: 원래 S19 대비','',
 '| 사례 | 옵션 | 기준 ms | 후보 ms | 시간 단축률 | 후보/기준 95% 구간 | 최종 반복 |','|---|---|---:|---:|---:|---|---:|']
def line(r):
    name=r['case'].replace('-growing-gap01','').replace('-fixed-gap01','').replace('-warm-gap01','')
    if r['censored']:return f'| {name} | {r["variant"]["id"]} | — | — | cap, 미수렴 | — | {r["diagnostics"]["iterations"]} |'
    lo,hi=r['ratio_bootstrap95']
    return f'| {name} | {r["variant"]["id"]} | {r["baseline_ms"]:.3f} | {r["candidate_ms"]:.3f} | {r["time_reduction_percent"]:+.2f}% | {lo:.3f}–{hi:.3f} | {r["diagnostics"]["iterations"]} |'
for r in rows:
    if '-growing-' in r['case']:lines.append(line(r))
lines += ['','## 이미 완성된 트리의 풀이 시간','',
 '트리 구축을 제외하고 kernel 준비·메모리 할당·CFR·원본 인증은 포함한다. 기준은 LCFR geometric이다. 아래는 각 사례의 관측상 최소 시간이며 보편적인 최선 설정을 선택한 것은 아니다. 전체 150개 결과는 RESULTS.json과 runner artifact에 있다.','',
 '| 사례 | 옵션 | 기준 ms | 후보 ms | 시간 단축률 | 후보/기준 95% 구간 | 최종 반복 |','|---|---|---:|---:|---:|---|---:|']
for r in best:
    if r['phase']=='fixed':lines.append(line(r))
lines += ['','## Warm start의 독립 비교','',
 '기준도 동시 CFR이며 이 표는 기존 S19에 대한 단축률이 아니다. 이전 게임에서 온 값을 새 게임의 scale과 대체 regret 조건으로 재계산한다. T는 실제 실행 반복에 포함하지 않는다.','',
 '| 사례 | 옵션 | 기준 ms | 후보 ms | 시간 단축률 | 후보/기준 95% 구간 | 최종 반복 |','|---|---|---:|---:|---:|---|---:|']
for r in rows:
    if '-warm-' in r['case']:lines.append(line(r))
lines += ['','## 구현 및 재현','',
 f'[옵션과 논문 대응](<{(R/"source/engine/benchmarks/s26_papers/README.md").as_posix()}>) · [실험 plan](<{(R/"source/engine/benchmarks/s26_papers/plan.json").as_posix()}>) · [기계 조회 결과](<{(R/"RESULTS.json").as_posix()}>)','',
 '정책 재사용은 새 게임에서 원본 gap을 재검사해 통과할 때만 풀이를 생략한다. warm 초기화는 simultaneous CFR에만 허용한다. 행렬 특례는 제한된 단일 결정 게임, 일반 sequence kernel은 완전 기억을 보존한 다중 턴 게임용이다. 두 경로 모두 최종 인증은 압축하지 않은 원본 Tree에서 수행한다.','',
 '로컬은 정확성 검사만 실행했다. 생산 미채택, 기본 OFF, AVX2 불필요. 단일 러너 결과이므로 작은 차이는 재현 확인이 필요하며 이번 수치로 승률 향상을 주장하지 않는다.']
(R/'RESULTS.md').write_text('\n'.join(lines)+'\n',encoding='utf-8')
print(json.dumps({k:v[k] for k in ['source_sha','ci_success','independent_lp_certificates','eligible_timing_pairs','censored_timing_pairs','core_tests']},indent=2))
