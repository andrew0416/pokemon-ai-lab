"""Pin downloaded evidence, separate profile diagnosis from clean speed results."""
from pathlib import Path
import hashlib,json,re,sys
from datetime import datetime
R=Path(__file__).resolve().parent;PUB=R/'publication';run=int(sys.argv[1])
receipt=json.loads((PUB/'receipt.json').read_bytes())
folder=PUB/f'run-{run}'/f's26h-phases-{run}-1'
download=json.loads((folder/'download-receipt.json').read_bytes())
def blob(name):
    b=(folder/name).read_bytes();assert hashlib.sha256(b).hexdigest()==download['files'][name];return b
def read(name):return json.loads(blob(name))
statuses=[json.loads(p.read_bytes()) for p in sorted(PUB.glob('status-*.json'))]
status=[s for s in statuses if s.get('id')==run][-1]
assert status['status']=='completed' and status['conclusion']=='success'
assert status['head_sha']==receipt['source_sha']==blob('source-sha.txt').decode().strip()
check=read('check/summary.json');pc=read('profile-check/summary.json');speed=read('paired/summary.json');profile=read('profile/summary.json');oracle=read('pipeline-lp/summary.json')
assert all(s['passed'] for s in (check,pc,speed,profile,oracle))
assert all(s['source_sha']==receipt['source_sha'] for s in (check,pc,speed,profile))
assert pc['cross_build_parity'] and speed['cross_build_parity'] and profile['cross_build_parity']
assert check['witnesses']==pc['witnesses']==speed['witnesses']==profile['witnesses']
assert len(check['witnesses'])==56 and len(speed['results'])==64 and len(profile['profiles'])==56
assert oracle['independent_lp'] and oracle['certificates']==652
for s in profile['profiles']:
    assert s['samples']==5
    assert abs(sum(p['aggregate_exclusive_percent'] for p in s['phases'])-100)<1e-9
rows=speed['results'];active=[r for r in rows if not r['censored']]
assert all(len(r['pairs'])==9 for r in active)
controls=[r for r in active if r['variant']['id'].endswith('-aa')]
assert len(controls)==16
bywork={}
for r in controls:
    bywork.setdefault(r['workload'],[]).append(r)
for r in active:
    lo,hi=r['ratio_bootstrap95']
    # Descriptive conservative screen, not a calibrated multiplicity correction.
    aa_band=max(abs(bound-1) for c in bywork[r['workload']] for bound in c['ratio_bootstrap95'])
    r['same_workload_aa_band_percent']=100*aa_band
    r['outside_aa_envelope']=hi<1-aa_band or lo>1+aa_band
    r['within_run_direction']='faster' if hi<1 else 'slower' if lo>1 else 'unresolved'
    r['mechanism_active']=r['diagnostics']['growth']['solves']>1 if r['variant']['id'] in ('index','owned','delta') else True
tests={}
for name in ('instrumented-tests.log','uninstrumented-tests.log','default-off.log'):
    found=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed',blob(name).decode())
    assert found and all(int(f)==0 for _,f in found)
    tests[name]=[int(n) for n,_ in found]
assert tests=={'instrumented-tests.log':[117],'uninstrumented-tests.log':[115],'default-off.log':[17]}
seconds=(datetime.fromisoformat(status['updated_at'].replace('Z','+00:00'))-datetime.fromisoformat(status['run_started_at'].replace('Z','+00:00'))).total_seconds()
value=dict(schema_version=1,source_sha=receipt['source_sha'],parent_sha=receipt['parent'],run_id=run,run_url=status['html_url'],
    ci_success=True,elapsed_ci_seconds=seconds,tests=tests,independent_lp_certificates=652,certificate_capped=652-oracle['converged'],
    cross_build_result_work_witnesses=56,profile_workloads=8,profile_configuration_captures=56,profile_repetitions_each=5,
    speed_conditions=24,speed_comparisons=64,eligible=len(active),censored=64-len(active),aa_controls=16,
    instrumented_for_diagnosis_only=True,default_off=True,production_engine_unchanged=True,search_algorithm_unchanged=True,
    production_adopted=False,local_speed_measured=False,win_rate_evaluated=False,portable_cpu='x86-64',
    artifact_directory=folder.as_posix(),speed=rows,profiles=profile['profiles'],
    aa_apparent_differences=sum(r['within_run_direction']!='unresolved' for r in controls),
    limitations=['one runner session; eight workload settings from six fixtures, not eight independent games',
      'phase clock/ledger overhead and separate-build code generation affect absolute profile times',
      'inclusive spans overlap; only exclusive costs sum to compute',
      'phase medians and speed paired ratios describe different experiments; do not infer speedups from profile percentages',
      'AA envelope is a descriptive screen, not a statistical multiple-testing correction',
      'returned-result drop measured after diagnostic inspection; headline compute excludes it',
      'no per-variant peak allocation/memory and no actual non-AVX2 machine run',
      'cross-build FNV full-result witnesses are noncryptographic regression checks; artifact files use SHA256'])
with (R/'RESULTS.json').open('x',encoding='utf-8') as f:json.dump(value,f,ensure_ascii=False,indent=2);f.write('\n')
lines=['# S26h 단계 비용과 실행 경로 대조 실험','',f'[GitHub 실행]({value["run_url"]}) · source `{value["source_sha"]}` · {seconds:.0f}초','',
    '알고리즘을 바꾸지 않고 새 실험 복사본에 계측과 대조군을 추가했다. 속도는 계측 코드가 컴파일되지 않은 동일 범용 x86-64 실행파일의 9개 ABBA/BAAB 블록이다. 단계 비용은 별도 계측 실행파일의 5회 회전 실행이다. 두 종류의 시간을 섞어 가속률을 계산하지 않는다.','',
    f'계측 ON 117개·OFF 115개·기본 설정 17개 테스트, 기존 652건 독립 BR/LP 인증 검사가 통과했다. 그중 {value["certificate_capped"]}건은 반복 상한에서 미수렴이며 인증 수치만 일치한 사례다. 8개 조건×7개 구성의 전체 트리/정책/작업량 지문 56개가 빌드 사이에 일치했다. 기존 fixture 6개의 재검사이며 새 독립 게임 652개가 아니다.','',
    '## 기존 경로의 단계별 비용','',
    '각 칸은 다섯 계측 실행의 총 시간 대비 해당 exclusive 시간 비율이다. 전이는 호출자 대기 시간을 포함하며 worker CPU 시간을 합산하지 않는다. admission 나머지는 행동 생성·평가·관측 문자열·raw 트리 생성 등이다.','',
    '| 조건 | 전이 | admission 나머지 | 컴파일 | kernel | CFR | 원본 인증 | scores | 선택 | 기타 |',
    '|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|']
workloads=list(bywork)
for w in workloads:
    r=next(r for r in profile['profiles'] if r['case']==w and r['variant']=='original')
    p={x['phase']:x['aggregate_exclusive_percent'] for x in r['phases']}
    selected=['transitions','admission_other','compile','kernel','cfr','certificate','scores','selection']
    vals=[p[k] for k in selected]+[sum(v for k,v in p.items() if k not in selected)]
    lines.append('| '+w+' | '+' | '.join(f'{x:.2f}%' for x in vals)+' |')
lines+=['','## 비계측 속도: 기준을 분리한 비교','',
    '양수는 시간 감소, 음수는 증가. 단축률은 각 블록 후보/기준 비율의 중앙값으로, 별도 시간 중앙값의 비율과 다를 수 있다. A/A도 유한 표본 오차가 있다.','',
    '| 조건 | 원본 A/A | 신경로 A/A | 원본→새 OFF | OFF→index | OFF→owned | owned→delta | OFF→checks | OFF→전체 |',
    '|---|---:|---:|---:|---:|---:|---:|---:|---:|']
for w in workloads:
    vals=[]
    for name in ('original-aa','new-off-aa','new-off','index','owned','delta','compressed','all-four'):
        r=next(r for r in rows if r['workload']==w and r['variant']['id']==name)
        vals.append('cap' if r['censored'] else f'{r["time_reduction_percent"]:+.2f}%')
    lines.append('| '+w+' | '+' | '.join(vals)+' |')
lines+=['','## 모든 비교와 구간','',
    '| 조건 | 후보 | 기준 ms | 후보 ms | 단축률 | 비율 95% 구간 | A/A 범위 밖 |',
    '|---|---|---:|---:|---:|---|---|']
for r in rows:
    if r['censored']:lines.append(f'| {r["workload"]} | {r["variant"]["id"]} | cap | cap | — | — | — |');continue
    lo,hi=r['ratio_bootstrap95']
    lines.append(f'| {r["workload"]} | {r["variant"]["id"]} | {r["baseline_ms"]:.3f} | {r["candidate_ms"]:.3f} | {r["time_reduction_percent"]:+.2f}% | {lo:.4f}–{hi:.4f} | {r["outside_aa_envelope"]} |')
lines+=['','## 전체 ON의 계측 상세','',
    '호출 횟수는 컴파일=해결 스냅샷 수, CFR=반복수 합계, 원본 인증=assessment 수와 대조했다. 개별 행 inclusive 합계는 중복된다. 개별 실행의 exclusive 합계만 전체 compute와 정확히 일치한다. 아래는 exclusive 중앙값으로 열의 합은 전체 중앙값과 다를 수 있다.','',
    '| 조건 | 스냅샷 | compute ms | 전이 ms | admission ms | 컴파일 ms | kernel ms | CFR ms | 인증 ms | 별도 반환물 해제 ms |',
    '|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|']
for w in workloads:
    r=next(r for r in profile['profiles'] if r['case']==w and r['variant']=='all-four');p={x['phase']:x for x in r['phases']}
    vals=[r['compute_ms']]+[p[k]['exclusive_ms'] for k in ('transitions','admission_other','compile','kernel','cfr','certificate')]+[r['returned_result_drop_ms']]
    lines.append(f'| {w} | {r["diagnostics"]["growth"]["solves"]} | '+' | '.join(f'{x:.3f}' for x in vals)+' |')
lines+=['','단일 러너의 64개 비교이며 다중 비교 보정 없는 탐색적 결과다. A/A 범위 밖 표시는 같은 조건의 두 A/A 구간보다 큰지를 본 보수적 설명 도구일 뿐 유의성 검정이 아니다. 반복되는 개선인지 별도 세션에서 확인해야 한다. 모든 최적화는 기본 OFF, 생산 엔진 미적용 상태로 보존했다.','',
    f'[기계 조회 결과](<{(R/"RESULTS.json").as_posix()}>) · [측정 정의](<{(R/"source/engine/benchmarks/s26_phases/README.md").as_posix()}>)']
(R/'RESULTS.md').write_text('\n'.join(lines)+'\n',encoding='utf-8')
print(json.dumps({k:value[k] for k in ('run_url','elapsed_ci_seconds','tests','independent_lp_certificates','eligible','aa_apparent_differences')},indent=2))
