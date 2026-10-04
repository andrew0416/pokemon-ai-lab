"""Evidence-pinned S24 follow-ups; diagnostics never substitute for clean timings."""
from pathlib import Path
import hashlib,json,re,statistics,sys
from datetime import datetime
R=Path(__file__).resolve().parent;PUB=R/'publication';run=int(sys.argv[1])
receipt=json.loads((PUB/'receipt.json').read_bytes());folder=PUB/f'run-{run}'/f's26i-audit-{run}-1'
download=json.loads((folder/'download-receipt.json').read_bytes())
def blob(name):
 b=(folder/name).read_bytes();assert hashlib.sha256(b).hexdigest()==download['files'][name];return b
def read(name):return json.loads(blob(name))
status=[json.loads(p.read_bytes()) for p in sorted(PUB.glob('status-*.json')) if json.loads(p.read_bytes()).get('id')==run][-1]
assert status['status']=='completed' and status['conclusion']=='success'
assert status['head_sha']==receipt['source_sha']==blob('source-sha.txt').decode().strip()
check=read('check/summary.json');pc=read('profile-check/summary.json');audit=read('audit-check/summary.json')
speed=read('paired/summary.json');profile=read('profile/summary.json');oracle=read('pipeline-lp/summary.json');memory=read('memory/summary.json');port=read('portability/summary.json')
assert all(s['passed'] for s in (check,pc,audit,speed,profile,oracle,memory,port))
assert all(s['source_sha']==receipt['source_sha'] for s in (check,pc,audit,speed,profile,memory,port))
assert all(s['cross_build_parity'] for s in (pc,audit,speed,profile))
assert check['witnesses']==pc['witnesses']==audit['witnesses']==profile['witnesses']
assert all(check['witnesses'][k]==v for k,v in speed['witnesses'].items())
assert len(check['witnesses'])==56 and len(speed['results'])==16 and len(profile['profiles'])==56
assert oracle['independent_lp'] and oracle['certificates']==1996 and oracle['distinct_growing_games']==64
assert memory['configuration_processes']==168 and not memory['local_accounting_check'] and not memory['timing']
assert port['emulated'] and not port['actual_old_hardware'] and not port['timing']
assert port['cpuid']==dict(avx=False,avx2=False,sse2=True) and port['engine_configurations']==6 and port['scalar_legacy_equal']
for p in profile['profiles']:
 assert p['samples']==5 and len(p['phases'])==19
 assert abs(sum(x['aggregate_exclusive_percent'] for x in p['phases'])-100)<1e-9
rows=speed['results'];active=[r for r in rows if not r['censored']];controls=[r for r in active if r['variant']['id'].endswith('-aa')]
assert all(len(r['pairs'])==15 for r in active) and len(controls)==4
for r in active:
 lo,hi=r['ratio_bootstrap95'];r['within_run_direction']='faster' if hi<1 else 'slower' if lo>1 else 'unresolved'
 band=max(abs(v-1) for c in controls if c['workload']==r['workload'] for v in c['ratio_bootstrap95'])
 r['same_workload_aa_band_percent']=100*band;r['outside_aa_envelope']=hi<1-band or lo>1+band
tests={}
for name in ('instrumented-tests.log','uninstrumented-tests.log','default-off.log'):
 found=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed',blob(name).decode());assert found and all(int(f)==0 for _,f in found);tests[name]=[int(n) for n,_ in found]
assert tests=={'instrumented-tests.log':[119],'uninstrumented-tests.log':[115],'default-off.log':[17]}
mem=[]
for w in [p['case'] for p in profile['profiles'] if p['variant']=='original']:
 for variant in ('original','new-off','index','owned','delta','compressed','all-four'):
  r=[x for x in memory['rows'] if x['case']==w and x['variant']==variant];assert len(r)==3
  keys=['allocations','reallocations','requested_growth_bytes','peak_above_baseline_bytes','returned_live_above_baseline_bytes','result_drop_freed_bytes','retained_after_result_drop_bytes','process_peak_rss_kib']
  mem.append(dict(case=w,variant=variant,medians={k:statistics.median(x[k] for x in r) for k in keys},cache=r[0]['cache']))
audit_counts={k:sum(x[k] for x in audit['audits']) for k in ('compiled_snapshots','sequence_snapshots','contractions','gap_checks')}
seconds=(datetime.fromisoformat(status['updated_at'].replace('Z','+00:00'))-datetime.fromisoformat(status['run_started_at'].replace('Z','+00:00'))).total_seconds()
value=dict(schema_version=1,source_sha=receipt['source_sha'],parent_sha=receipt['parent'],run_id=run,run_url=status['html_url'],ci_success=True,elapsed_ci_seconds=seconds,
 tests=tests,independent_lp_certificates=1996,certificate_capped=1996-oracle['converged'],distinct_fixed_games=oracle['distinct_fixed_games'],distinct_growing_games=64,
 cross_build_result_work_witnesses=56,intermediate_audits=audit_counts,snapshot_generator_seeds=64,
 speed_comparisons=16,eligible=len(active),censored=16-len(active),aa_controls=4,paired_rounds=15,
 profiles=profile['profiles'],speed=rows,memory=mem,memory_processes=168,portability=port,
 instrumented_for_diagnosis_only=True,default_off=True,production_engine_unchanged=True,search_algorithm_unchanged=True,production_adopted=False,local_speed_measured=False,win_rate_evaluated=False,
 artifact_directory=folder.as_posix(),limitations=['Bounded fixture corpus, not exhaustive future-mechanic proof','Separate build diagnostic clocks and global allocator perturb execution; use clean paired speed only for gains','Requested Rust heap bytes and whole-process RSS are different quantities','Intermediate shadow checks are diagnostic guards, not a general mutable-tree production interface','Emulated Nehalem execution is not real old-hardware timing','15-block single-session intervals are exploratory, no multiplicity correction'])
with (R/'RESULTS.json').open('x',encoding='utf-8') as f:json.dump(value,f,ensure_ascii=False,indent=2);f.write('\n')
lines=['# S26i 세부 비용·메모리·중간 상태·호환성 검증','',f'[GitHub 실행]({value["run_url"]}) · `{value["source_sha"]}` · {seconds:.0f}초','',
f'119개 계측/중간 상태 테스트, 115개 일반 테스트, 기본 설정 17개 통과. 독립 BR/LP 인증 1,996건 중 {value["certificate_capped"]}건은 반복 상한 미수렴이며 인증 수치만 일치했다. 새로운 seed의 유한 게임 29개와 관측 게임 64개를 여러 설정·부분 탐색으로 검사한 수로, 독립 게임 1,996개를 뜻하지 않는다.','',
'## 세부 비용','', '별도 진단 빌드의 exclusive 시간 비율. 19개 항목의 합은 매 실행 compute와 일치한다. 전이는 호출자 wall time이며 worker CPU 합계가 아니다. 키 생성은 leaf의 키만 별도 계측하며 expand에서의 키 생성은 menu에 포함된다. 절대 비율을 실제 가속률로 해석하지 않는다.','',
'| 조건 | 전이 | 관측 | 이력 복사 | 이력 키 | 경계 등록 | 행동/메뉴 | 평가 | 기타 |','|---|---:|---:|---:|---:|---:|---:|---:|---:|']
for row in profile['profiles']:
 if row['variant']!='original':continue
 p={x['phase']:x['aggregate_exclusive_percent'] for x in row['phases']};keys=['transitions','observation','history_copy','history_keys','boundary_mark','menu','leaf_evaluation']
 lines.append('| '+row['case']+' | '+' | '.join(f'{p[k]:.2f}%' for k in keys)+f' | {sum(v for k,v in p.items() if k not in keys):.2f}% |')
lines+=['','## 많은 스냅샷 재측정','', '동일 바이너리의 15개 ABBA/BAAB 블록. 양수는 시간 감소. 구간은 후보/기준 시간 비율. A/A 범위 표시는 설명 도구이며 통계 보정이 아니다.','', '| 조건 | 후보 | 단축률 | 비율 95% 구간 | A/A 범위 밖 |','|---|---|---:|---|---|']
for r in rows:
 if r['censored']:lines.append(f'| {r["workload"]} | {r["variant"]["id"]} | cap | — | — |');continue
 lo,hi=r['ratio_bootstrap95'];lines.append(f'| {r["workload"]} | {r["variant"]["id"]} | {r["time_reduction_percent"]:+.2f}% | {lo:.4f}–{hi:.4f} | {r["outside_aa_envelope"]} |')
lines+=['','## 메모리','', '8조건 × 7구성 × 별도 프로세스 3회. 아래는 중앙값. heap peak는 초기 입력·스레드풀을 제외한 요청 바이트 증가량이며 RSS는 그것들을 포함한다. allocator의 캐시·메타데이터까지 heap counter가 측정하지 않는다.','', '| 조건 | 구성 | 할당 수 | 누적 요청 MiB | heap peak MiB | 반환물 MiB | 반환물 해제 후 KiB | 프로세스 RSS MiB |','|---|---|---:|---:|---:|---:|---:|---:|']
for r in mem:
 if r['variant'] not in ('new-off','all-four'):continue
 m=r['medians'];lines.append(f'| {r["case"]} | {r["variant"]} | {m["allocations"]:,} | {m["requested_growth_bytes"]/2**20:.2f} | {m["peak_above_baseline_bytes"]/2**20:.2f} | {m["returned_live_above_baseline_bytes"]/2**20:.2f} | {m["retained_after_result_drop_bytes"]/1024:.2f} | {m["process_peak_rss_kib"]/1024:.2f} |')
lines+=['','## 중간 상태와 비AVX2','',f'엔진 56구성의 최종 지문은 비계측·계측·shadow 빌드에서 일치. 중간 검사 합계: {audit_counts}. 추가 64 seed leaf replacement 코퍼스는 정보 ID 재배치, 효용 scale 변경, 0·극소 chance와 이전 비말단 불변 계약을 검사한다. 일반 가변 트리 지원을 약속하지 않는다.','',
f'{port["qemu"]}, Nehalem CPU 모델에서 CPUID AVX=false, AVX2=false, SSE2=true를 확인한 뒤 엔진 2조건×3구성과 기존 스칼라 finite-tree 경로를 실제 실행했다. native 결과와 전체 JSON이 일치했다. 실제 구형 하드웨어 시험이나 해당 하드웨어의 속도 측정은 아니다.','',
'## 다음 구현','', '이력 키를 매번 처음부터 Debug 포맷하는 비용이 8조건에서 11~14%다. S25에서 부모 키의 prefix를 복사하고 새 행동·관측 두 항목만 기존 Memory Debug 형식으로 붙이는 후보를 구현한다. 관측이나 평가 함수를 캐시하지 않고 공용 이력의 정렬·정보 구분도 보존한다. 속도와 메모리는 별도 비계측/계측 실행으로 판단한다.','',
'모든 옵션은 기본 OFF이며 생산 엔진에는 적용하지 않았다. 승률을 평가하지 않았다.']
(R/'RESULTS.md').write_text('\n'.join(lines)+'\n',encoding='utf-8')
print(json.dumps(dict(run_url=value['run_url'],seconds=seconds,tests=tests,certificates=1996,capped=value['certificate_capped'],audits=audit_counts,speed=[{k:r[k] for k in ('workload','variant','time_reduction_percent','ratio_bootstrap95')} for r in active if r['variant']['id'] in ('owned','all-four')]),indent=2))
