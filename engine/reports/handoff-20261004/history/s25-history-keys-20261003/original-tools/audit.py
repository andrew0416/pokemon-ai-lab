"""Pin the encoded-history results; compare paired variants, not separate sessions."""
from pathlib import Path
import hashlib,json,re,statistics,sys
from datetime import datetime
R=Path(__file__).resolve().parent;PUB=R/'publication';run=int(sys.argv[1])
receipt=json.loads((PUB/'receipt.json').read_bytes());folder=PUB/f'run-{run}'/f's26j-history-{run}-1'
download=json.loads((folder/'download-receipt.json').read_bytes())
def blob(name):
 b=(folder/name).read_bytes();assert hashlib.sha256(b).hexdigest()==download['files'][name];return b
def read(name):return json.loads(blob(name))
status=[json.loads(p.read_bytes()) for p in sorted(PUB.glob('status-*.json')) if json.loads(p.read_bytes()).get('id')==run][-1]
assert status['status']=='completed' and status['conclusion']=='success'
assert status['head_sha']==receipt['source_sha']==blob('source-sha.txt').decode().strip()
check=read('check/summary.json');pc=read('profile-check/summary.json');audit=read('audit-check/summary.json');speed=read('paired/summary.json');profile=read('profile/summary.json');oracle=read('pipeline-lp/summary.json');memory=read('memory/summary.json');port=read('portability/summary.json')
assert all(s['passed'] for s in (check,pc,audit,speed,profile,oracle,memory,port))
assert all(s['source_sha']==receipt['source_sha'] for s in (check,pc,audit,speed,profile,memory,port))
assert all(s['cross_build_parity'] for s in (pc,audit,speed,profile))
assert check['witnesses']==pc['witnesses']==audit['witnesses']==profile['witnesses']
assert all(check['witnesses'][k]==v for k,v in speed['witnesses'].items())
assert len(check['witnesses'])==40 and len(speed['results'])==32 and len(profile['profiles'])==40
assert oracle['independent_lp'] and oracle['certificates']==1869 and oracle['distinct_growing_games']==32
assert memory['configuration_processes']==120 and not memory['local_accounting_check'] and not memory['timing']
assert port['cpuid']==dict(avx=False,avx2=False,sse2=True) and port['engine_configurations']==10 and port['scalar_legacy_equal'] and port['emulated'] and not port['actual_old_hardware']
for p in profile['profiles']:
 assert p['samples']==5 and len(p['phases'])==19
 assert abs(sum(x['aggregate_exclusive_percent'] for x in p['phases'])-100)<1e-9
rows=speed['results'];active=[r for r in rows if not r['censored']];controls=[r for r in active if r['variant']['id'].endswith('-aa')]
assert all(len(r['pairs'])==9 for r in active) and len(controls)==16
for r in active:
 lo,hi=r['ratio_bootstrap95'];r['within_run_direction']='faster' if hi<1 else 'slower' if lo>1 else 'unresolved'
 aa=next(c for c in controls if c['case']==r['case'])
 band=max(abs(v-1) for v in aa['ratio_bootstrap95']);r['same_path_aa_band_percent']=100*band;r['outside_aa_envelope']=hi<1-band or lo>1+band
tests={}
for name in ('instrumented-tests.log','uninstrumented-tests.log','default-off.log'):
 found=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed',blob(name).decode());assert found and all(int(f)==0 for _,f in found);tests[name]=[int(n) for n,_ in found]
assert tests=={'instrumented-tests.log':[121],'uninstrumented-tests.log':[117],'default-off.log':[17]}
workloads=[p['case'] for p in profile['profiles'] if p['variant']=='original'];mem=[]
for w in workloads:
 for variant in ('original','new-off','history','all-four','all-history'):
  r=[x for x in memory['rows'] if x['case']==w and x['variant']==variant];assert len(r)==3
  keys=['allocations','reallocations','requested_growth_bytes','peak_above_baseline_bytes','returned_live_above_baseline_bytes','result_drop_freed_bytes','retained_after_result_drop_bytes','process_peak_rss_kib']
  mem.append(dict(case=w,variant=variant,medians={k:statistics.median(x[k] for x in r) for k in keys},cache=r[0]['cache']))
audit_counts={k:sum(x[k] for x in audit['audits']) for k in ('compiled_snapshots','sequence_snapshots','contractions','gap_checks')}
seconds=(datetime.fromisoformat(status['updated_at'].replace('Z','+00:00'))-datetime.fromisoformat(status['run_started_at'].replace('Z','+00:00'))).total_seconds()
value=dict(schema_version=1,source_sha=receipt['source_sha'],parent_sha=receipt['parent'],run_id=run,run_url=status['html_url'],ci_success=True,elapsed_ci_seconds=seconds,tests=tests,
 independent_lp_certificates=1869,certificate_capped=1869-oracle['converged'],distinct_fixed_games=29,distinct_growing_games=32,observed_exact_off_on_pairs=32*3*8+1,
 cross_build_result_work_witnesses=40,intermediate_audits=audit_counts,callback_combinations=36,cursor_prefix_depth=128,cursor_label_families=5,
 speed_comparisons=32,eligible=len(active),censored=32-len(active),aa_controls=16,paired_rounds=9,profiles=profile['profiles'],speed=rows,memory=mem,memory_processes=120,portability=port,
 default_off=True,production_engine_unchanged=True,search_algorithm_unchanged=True,storage_representation_changed=True,production_adopted=False,local_speed_measured=False,win_rate_evaluated=False,artifact_directory=folder.as_posix(),
 limitations=['One runner session; eight settings from six engine fixtures','Same-binary OFF/ON ratios do not measure zero cross-version OFF enum/layout overhead','Diagnostics have clock/global allocator overhead and are not speed evidence','Encoded event formatting moves into history_copy; do not infer total work from history_keys alone','Full exported-game equality and bounded oracles do not prove every future mechanic','QEMU Nehalem is emulation, not actual old hardware','No win rate or production adoption; no multiplicity correction'])
with (R/'RESULTS.json').open('x',encoding='utf-8') as f:json.dump(value,f,ensure_ascii=False,indent=2);f.write('\n')
lines=['# S26j 증분 이력 키 최적화 실험','',f'[GitHub 실행]({value["run_url"]}) · `{value["source_sha"]}` · {seconds:.0f}초','',
'부모의 private-history 문자열을 이어 써서 과거 관측을 다시 Debug 포맷하지 않는 저장 방식이다. 새 행동·관측만 같은 Memory Debug 구현으로 포맷한다. 각 플레이어의 정보, public history 순서, 콜백 호출, 탐색·인증 알고리즘은 그대로다. `pipeline.encoded_history`는 기본 OFF다.','',
f'121개 계측/중간 상태 테스트, 일반 117개·기본 17개 통과. 새 seed와 escaped label을 쓰는 관측 게임 32개에서 769개 전체 OFF/ON JSON 쌍을 비교했다. 독립 BR/LP 인증 1,869건 중 {value["certificate_capped"]}건은 cap 미수렴이며 인증 수치만 일치했다. 횟수는 여러 설정/부분 탐색을 포함하며 독립 게임 1,869개가 아니다. 엔진 40구성의 전체 결과·작업량 지문이 각 빌드와 일치했다.','',
'## 비계측 속도','', '각 행은 같은 바이너리·같은 설정에서 이력 옵션만 바꾼 9개 ABBA/BAAB 블록이다. 양수는 시간 단축. 추가 적용의 기준은 기존 4개 옵션 전체 ON이며, 두 열의 단축률을 더하면 안 된다.','',
'| 조건 | 단독 적용 | 비율 95% 구간 | 기존 전체 ON에 추가 | 비율 95% 구간 |','|---|---:|---|---:|---|']
for w in workloads:
 cells=[]
 for variant in ('history','all-history'):
  r=next(r for r in rows if r['workload']==w and r['variant']['id']==variant)
  if r['censored']:cells.extend(['cap','—']);continue
  lo,hi=r['ratio_bootstrap95'];cells.extend([f'{r["time_reduction_percent"]:+.2f}%',f'{lo:.4f}–{hi:.4f}'])
 lines.append('| '+w+' | '+' | '.join(cells)+' |')
lines+=['','## A/A와 모든 비교','', '| 조건 | 후보 | 기준 ms | 후보 ms | 단축률 | 비율 구간 | A/A 범위 밖 |','|---|---|---:|---:|---:|---|---|']
for r in active:
 lo,hi=r['ratio_bootstrap95'];lines.append(f'| {r["workload"]} | {r["variant"]["id"]} | {r["baseline_ms"]:.3f} | {r["candidate_ms"]:.3f} | {r["time_reduction_percent"]:+.2f}% | {lo:.4f}–{hi:.4f} | {r["outside_aa_envelope"]} |')
lines+=['','## 메모리','', '각 구성 별도 프로세스 3회 중앙값. 요청 heap은 baseline 위 peak, RSS는 입력/풀/allocator를 포함하는 프로세스 peak다.','', '| 조건 | 구성 | 할당 수 | 누적 요청 MiB | heap peak MiB | 반환물 MiB | RSS MiB |','|---|---|---:|---:|---:|---:|---:|']
for r in mem:
 if r['variant']=='original':continue
 m=r['medians'];lines.append(f'| {r["case"]} | {r["variant"]} | {m["allocations"]:,} | {m["requested_growth_bytes"]/2**20:.2f} | {m["peak_above_baseline_bytes"]/2**20:.2f} | {m["returned_live_above_baseline_bytes"]/2**20:.2f} | {m["process_peak_rss_kib"]/1024:.2f} |')
lines+=['','## 진단 비용과 호환성','', '새 이벤트 포맷은 history_copy로 이동한다. 따라서 history_keys 단일 항목의 감소를 전체 절감으로 오인하지 않는다. 아래는 합산한 exclusive 시간 중앙값이며 서로 다른 단계의 중앙값 합이다. 실제 가속률은 위 clean 측정만 사용한다.','',
'| 조건 | 구성 | history_copy+keys ms | menu ms | boundary ms | observation ms |','|---|---|---:|---:|---:|---:|']
for r in profile['profiles']:
 if r['variant'] not in ('new-off','history'):continue
 p={x['phase']:x['exclusive_ms'] for x in r['phases']};lines.append(f'| {r["case"]} | {r["variant"]} | {p["history_copy"]+p["history_keys"]:.3f} | {p["menu"]:.3f} | {p["boundary_mark"]:.3f} | {p["observation"]:.3f} |')
lines+=['',f'중간 대조: {audit_counts}. 비AVX2 QEMU Nehalem에서 CPUID를 확인한 뒤 두 국면×5구성과 기존 스칼라 경로가 native 전체 결과와 일치했다. 실제 구형 CPU 속도 측정은 아니다.','',
'실험 코드의 저장 표현은 바뀌었으므로 같은 바이너리 OFF/ON 비교만으로 이전 커밋 대비 OFF 비용이 0이라고 보장하지 않는다. 기존 스칼라 feature에는 encoded variant가 컴파일되지 않는다. 생산 엔진과 기본 설정은 유지했고 승률을 평가하지 않았다.','',
f'[구현·측정 계약](<{(R/"source/engine/benchmarks/s26_history/README.md").as_posix()}>)']
(R/'RESULTS.md').write_text('\n'.join(lines)+'\n',encoding='utf-8')
print(json.dumps(dict(run_url=value['run_url'],seconds=seconds,capped=value['certificate_capped'],speed=[dict(workload=r['workload'],variant=r['variant']['id'],reduction=r['time_reduction_percent'],interval=r['ratio_bootstrap95']) for r in active if not r['variant']['id'].endswith('-aa')]),indent=2))
