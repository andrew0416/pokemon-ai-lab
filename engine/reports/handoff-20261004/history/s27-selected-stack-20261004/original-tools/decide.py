"""Select one measured large stage; keep the unimplemented benefit conditional."""
from pathlib import Path
import hashlib,json,statistics
R=Path(__file__).resolve().parent;S=R/'source'
r=json.loads((R/'RESULTS.json').read_bytes())
plan=json.loads((S/'engine/benchmarks/s26_stack/plan.json').read_bytes())
threads={v['id']:v['threads'] for v in plan['cases']}
costs=[]
for row in r['profiles']:
    if row['variant']!='selected-five':continue
    phase={v['phase']:v for v in row['phases']}
    f=phase['observation']['aggregate_exclusive_percent']
    max_parallel=min(threads[row['case']],4,row['diagnostics']['growth']['attempted_transitions'])
    ideal=f*(1-1/max_parallel) if max_parallel>1 else 0.
    costs.append(dict(case=row['case'],threads=threads[row['case']],maximum_eligible_parallelism_bound=max_parallel,
        transition_wall_percent=phase['transitions']['aggregate_exclusive_percent'],
        observation_wall_percent=f,observation_calls=phase['observation']['calls'],
        ideal_observation_only_parallel_reduction_percent=ideal,
        required_observation_speed_factor_for_five_percent=1/(1-5/f) if threads[row['case']]>1 and f>5 else None))
parallel=[v for v in costs if v['threads']>1]
assert statistics.median(v['ideal_observation_only_parallel_reduction_percent'] for v in parallel)>5
anchors=[
    ('engine/search/src/bayesian/tree/engine/parallel.rs','//! Bounded immutable transition jobs.','Existing workers enumerate transitions only; observation is explicitly serial.'),
    ('engine/search/src/bayesian/tree/builder/growing.rs','self.b.domain.observation(&position)?','Every child is observed after the batch returns, before serial phase/history/evaluation.'),
    ('engine/search/src/bayesian/tree/engine.rs','fn snapshot_impl','The pure engine snapshot serializer has a single explicit disclosure allowlist.'),
    ('engine/search/src/bayesian/tree/engine/pipeline.rs','let batch = settings','Pool width >1 selects the immutable bounded batch; one-thread fallback exists.'),
    ('engine/search/src/bayesian/tree/phase_cost.rs','//! Only the calling thread is recorded.','Transition phase is submit-and-wait wall, not summed worker CPU.')]
evidence=[]
for rel,anchor,claim in anchors:
    p=S/rel;data=p.read_bytes();text=data.decode();assert anchor in text
    evidence.append(dict(path=p.resolve().as_posix(),sha256=hashlib.sha256(data).hexdigest(),anchor=anchor,
        line=text[:text.index(anchor)].count('\n')+1,claim=claim))
direct=r['direct'];reg=r['history_off_repeat']
regressions=[v for v in direct if v['within_run_direction']=='higher']
selected=dict(id='S26m-engine-observation-batch',
    title_ko='엔진 전용 관측 생성을 기존 전이 워커에 함께 배치',state='selected-for-isolated-experiment-not-implemented',
    whole_compute_target_percent=5,phase_costs=costs,evidence=evidence,
    rationale_ko='전이 워커가 반환된 뒤 호출자에서 모든 확률 자식의 관측 문자열을 순차 생성한다. 이 순수 엔진 관측만 기존 최대 4개 작업에 합치면 큰 직렬 구간을 병렬화할 여지가 있다.',
    implementation_plan=[
        'Create a new isolated S28 experiment from pinned S25 selected-five. Add an explicitly engine-only prepared-observation batch; default OFF.',
        'Reuse snapshot_impl and exactly the same phase text/disclosure allowlist. Each joint worker produces ending states plus per-child deferred observation Result. No generic observer/evaluator/prior callbacks move to workers.',
        'Consume joints and children in their original indexed order. Observe errors only when their child would have been visited. Transition errors, invalid chance mass, node limits, first error and atomic publication remain serial and in the old order.',
        'Keep pool width <=4, cap attempted transitions identically, preserve factored-mode propagation and input restoration. One-thread/no-pool route uses the existing serial implementation.',
        'Measure observation calls actually eligible for preparation, batch occupancy, bytes retained and deferred errors before claiming the whole phase moved. Do not call the ideal bound a forecast.'
    ],
    go_no_go=[
        'Exact full tree/solution/search-work parity for ON/OFF with 1/2/4 workers; errors, node/transition caps, switch closure, rollback and future synthetic observer cases included.',
        'A generic stateful domain/evaluator/prior keeps the original callback trace. Purity permission belongs only to the concrete engine snapshot adapter; no new Send/Sync requirement for generic Domain.',
        'Independent BR/LP and intermediate snapshot audits after any builder/interface change; preserved public/private partitions, source-registered observation fields and strict Illusion rejection.',
        'Clean same-runner selected-five vs selected-five+candidate, isolated targets and A/A; target >=5% complete-compute reduction on at least two preregistered nontrivial workloads with ratio interval entirely below 0.95.',
        'Include one-thread fallback, heldouts, return/live heap and peak RSS; do not promote a systematic regression or unbounded speculative observation storage.',
        'Generic x86-64 scalar build remains supported; no AVX2 dependency. No strength/win-rate experiment.'
    ],
    limitations=[
        'The projected f*(1-1/k), k<=4, is an optimistic Amdahl ceiling for the separately instrumented observation phase, not measured savings. The two-transition tiny case cannot fill four jobs.',
        'Root observations and serial fallbacks cannot all move. Unequal outcome counts, small batches, scheduler, allocator and result-transfer overhead reduce benefit.',
        'Precomputed strings may raise peak memory; discarded/capped speculative work must remain bounded and counted.',
        'This is not observation caching, history interning, direct_write, or the rejected borrowed public-boundary clone change; those costs/contracts differ.',
        'Production remains unchanged. Before promotion, unresolved S25 OFF-layout/codegen cost needs separate diagnosis.'
    ])
decision=dict(state='direct-stack-measured-large-bottleneck-selected',source_sha=r['source_sha'],run_id=r['run_id'],
    direct_reduction_range_percent=[min(v['time_reduction_percent'] for v in direct),max(v['time_reduction_percent'] for v in direct)],
    faster_intervals=sum(v['within_run_direction']=='lower' for v in direct),
    faster_outside_aa=sum(v['within_run_direction']=='lower' and v['outside_aa_envelope'] for v in direct),
    above_five_percent=sum(v['time_reduction_percent']>=5 for v in direct),
    history_off_repeat=reg,history_off_cause_resolved=False,direct_regressions=regressions,
    summary_ko='최근 유지 5개 옵션은 10조건 중 9조건에서 8.00~14.83% 단축됐다. 작은 메가 국면은 8.03% 느려져 보편적 개선이나 생산 채택으로 판정하지 않는다. 다음 후보는 엔진 전용 관측 생성의 배치 병렬화 하나로 좁힌다.',
    selected_next=selected,deferred=['S26k-private-key-view-candidate'],rejected_boundary_stays_rejected=True,
    production_adopted=False,default_off=True,wins_evaluated=False)
if (R/'DECISION.json').exists():
    drafts=R/'review-drafts';drafts.mkdir(exist_ok=True)
    for name in ('DECISION.json','DECISION.md'):
        with (drafts/name).open('xb') as f:f.write((R/name).read_bytes())
with (R/'DECISION.json').open('w',encoding='utf8') as f:json.dump(decision,f,ensure_ascii=False,indent=2);f.write('\n')
lines=['# S27 누적 결과와 다음 큰 병목','',decision['summary_ko'],'',
    f'직접 시간 단축 범위 {decision["direct_reduction_range_percent"][0]:+.2f}–{decision["direct_reduction_range_percent"][1]:+.2f}%. '
    f'비율 구간 1 아래 {decision["faster_intervals"]}/10, A/A 설명용 범위 밖 개선 {decision["faster_outside_aa"]}/10. '
    '프로젝트 초기부터의 모든 변경을 대표하는 수치가 아니다. [전체 결과](RESULTS.md).','',
    '## 선택: 엔진 전용 관측 생성의 배치 병렬화','',selected['rationale_ko'],'',
    '|조건|전이 wall %|관측 wall %|관측 호출|최대 4워커 이상적 전체 단축 상한 %|5% 달성에 필요한 관측 가속|',
    '|---|---:|---:|---:|---:|---:|']
for v in costs:
    need=v['required_observation_speed_factor_for_five_percent']
    lines.append(f'|{v["case"]}|{v["transition_wall_percent"]:.2f}|{v["observation_wall_percent"]:.2f}|{v["observation_calls"]}|{v["ideal_observation_only_parallel_reduction_percent"]:.2f}|'+(f'{need:.2f}배' if need else '직렬 fallback')+'|')
lines += ['', '이 상한은 관측 전체가 완벽히 k등분된다는 가정의 `f × (1−1/k)`, k≤4이다. 작은 메가 국면은 전이가 2개여서 k도 최대 2로 제한했으며, 관측만으로 목표 5%에 못 미친다. 실제 단축률이나 예측값이 아니다. 루트·직렬 호출, 작업 불균형, 추가 문자열 보관 때문에 낮아진다. phase는 계측 바이너리의 진단값이고 clean 속도와 섞지 않는다.', '',
    '## 보존할 계약','',
    '공통 snapshot 허용 목록과 원래 출력 문자열을 그대로 사용한다. 숨겨진 State를 관측 키로 사용하지 않는다. 일반 도메인의 관측·평가·prior는 상태를 가질 수 있으므로 기존 직렬 호출을 유지한다. 엔진의 순수 관측 경로만 별도 API로 허용한다.', '',
    '각 워커는 관측 성공/오류를 결과로 보관한다. 호출자는 기존 joint/자식 순서에서 확률 검증·노드 할당 후 원래 도달할 관측 결과만 소비한다. 먼저 발생해야 할 전이/상한 오류보다 나중 관측 오류를 일찍 반환하지 않는다. 부분 tree를 게시하지 않으며 factored 모드·입력 복원·시도 횟수도 유지한다.', '',
    '한 배치는 기존 최대 4개 joint로 제한하고, 1워커·pool 없음에는 기존 직렬 구현을 쓴다. 반환형과 저장 구조가 바뀌면 OFF의 비용도 새로 대조한다.', '',
    '## 실험 통과 기준','',
    '별도 S28에서만 구현한다. 1/2/4워커, 두 heldout, 오류·예산 경계·switch closure·rollback, 중간 snapshot과 독립 BR/LP를 검사한다. 전체 tree/solution/작업량을 유지한 채 최소 두 비자명한 사전 지정 조건에서 전체 compute 5% 이상 감소를 목표로 한다. clean 비율 구간도 0.95 아래여야 하고 A/A·1워커 회귀·peak heap/RSS를 함께 확인한다.', '',
    '## 작은 국면의 회귀','',
    'heldout-mega-synthesis는 8노드·2전이·1회 풀이의 아주 작은 게임이다. 0.273→0.292ms, paired 비율로 +8.03% 시간이 증가했고 구간 [1.0498, 1.1412]도 A/A 설명용 범위를 벗어났다. 작은 절대 차이라고 삭제하지 않는다. 한두 전이에서 상수 비용이 지배한다는 가설은 가능하지만, 어떤 옵션이나 codegen이 원인인지는 이 실험으로 분리할 수 없다. 다음에는 소스 버전 OFF 비교와 동일 바이너리 옵션 제거 비교를 분리하고 더 긴 sample 단위를 사전 지정해 확인한다. 이번에는 자동 휴리스틱이나 생산 채택을 추가하지 않는다.', '',
    '## 남는 제한','',
    f'이전 S25 OFF 지연의 이번 반복은 {100*(reg["ratio_median"]-1):+.2f}% 시간 변화, 비율 구간 {reg["ratio_bootstrap95"]}. '
    '이번 OFF 비율 구간은 1보다 높지만 A/A 범위 밖으로 완전히 벗어나지는 않았다. 구체적 History layout/codegen 원인은 분리하지 않았으므로 기존 과제는 열어 둔다. 공통 CLI의 결과 파괴가 다음 요청 전에 완료되도록 측정 경계를 강화한 것도 이전 실행과의 차이다.', '',
    '작은 private key view 후보는 연기하고, 이득이 입증되지 않은 S26 경계 복사 변경도 누적 조합에서 제외한다. 생산 엔진·KEEP_R1·P15는 변경하지 않았다. 기본 OFF와 비AVX2 scalar 경로를 유지한다. 승률 미평가.']
(R/'DECISION.md').write_text('\n'.join(lines)+'\n',encoding='utf8')
print(json.dumps(dict(selected=selected['id'],direct_range=decision['direct_reduction_range_percent'],observation_range=[min(v['observation_wall_percent'] for v in parallel),max(v['observation_wall_percent'] for v in parallel)]),ensure_ascii=False))
