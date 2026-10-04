"""Freeze runner evidence for boundary lookup, historical controls and sand RSS."""
from pathlib import Path
from datetime import datetime
import hashlib, json, re, statistics, sys

R = Path(__file__).resolve().parent
PUB = R / 'publication'
run = int(sys.argv[1])
receipt = json.loads((PUB / 'receipt.json').read_bytes())
folder = PUB / f'run-{run}' / f's26k-boundary-{run}-1'
download = json.loads((folder / 'download-receipt.json').read_bytes())

def blob(name):
    payload = (folder / name).read_bytes()
    assert hashlib.sha256(payload).hexdigest() == download['files'][name], name
    return payload

def read(name):
    return json.loads(blob(name))

def direction(row, aa):
    lo, hi = row['ratio_bootstrap95']
    assert lo <= row['ratio_median'] <= hi
    row['within_run_direction'] = 'lower' if hi < 1 else 'higher' if lo > 1 else 'unresolved'
    band = max(abs(v - 1) for v in aa['ratio_bootstrap95'])
    row['same_path_aa_band_percent'] = 100 * band
    row['outside_aa_envelope'] = hi < 1 - band or lo > 1 + band

statuses = [json.loads(p.read_bytes()) for p in sorted(PUB.glob('status-*.json'))]
status = [v for v in statuses if v.get('id') == run][-1]
assert status['status'] == 'completed' and status['conclusion'] == 'success'
assert status['head_sha'] == receipt['source_sha'] == blob('source-sha.txt').decode().strip()
check, pc, shadow, speed, profile, oracle, memory, port, cross, rss = [read(p + '/summary.json') for p in
    ('check', 'profile-check', 'audit-check', 'paired', 'profile', 'pipeline-lp', 'memory', 'portability', 'cross-version', 'rss-repeat')]
assert all(v['passed'] for v in (check, pc, shadow, speed, profile, oracle, memory, port, cross, rss))
assert all(v['source_sha'] == receipt['source_sha'] for v in (check, pc, shadow, speed, profile, memory, port, cross, rss))
assert all(v['cross_build_parity'] for v in (pc, shadow, speed, profile))
assert check['witnesses'] == pc['witnesses'] == shadow['witnesses'] == profile['witnesses']
assert all(check['witnesses'][k] == v for k, v in speed['witnesses'].items())
assert len(check['witnesses']) == len(profile['profiles']) == 56
assert oracle['independent_lp'] and oracle['certificates'] == 3405 and oracle['distinct_growing_games'] == 32
assert len(speed['results']) == 48 and memory['configuration_processes'] == 168
assert not memory['local_accounting_check'] and not memory['timing']
assert port['cpuid'] == dict(avx=False, avx2=False, sse2=True)
assert port['engine_configurations'] == 14 and port['scalar_legacy_equal']
assert port['emulated'] and not port['actual_old_hardware']
for row in profile['profiles']:
    assert row['samples'] == 5 and len(row['phases']) == 19
    assert abs(sum(p['aggregate_exclusive_percent'] for p in row['phases']) - 100) < 1e-9

rows = speed['results']
active = [v for v in rows if not v['censored']]
controls = [v for v in active if v['variant']['id'].endswith('-aa')]
assert len(controls) == 24 and all(len(v['pairs']) == 9 for v in active)
for row in active:
    direction(row, next(v for v in controls if v['case'] == row['case']))

tests = {}
for name in ('instrumented-tests.log', 'uninstrumented-tests.log', 'default-off.log'):
    found = re.findall(r'test result: ok\. (\d+) passed; (\d+) failed', blob(name).decode())
    assert found and all(int(f) == 0 for _, f in found)
    tests[name] = [int(n) for n, _ in found]
assert tests == {'instrumented-tests.log': [122], 'uninstrumented-tests.log': [118], 'default-off.log': [17]}

workloads = [v['case'] for v in profile['profiles'] if v['variant'] == 'original']
assert len(workloads) == 8 and len(set(workloads)) == 8
mem = []
for workload in workloads:
    for variant in ('original', 'new-off', 'boundary', 'history', 'history-boundary', 'all-history', 'all-six'):
        found = [v for v in memory['rows'] if v['case'] == workload and v['variant'] == variant]
        assert len(found) == 3
        keys = ('allocations', 'reallocations', 'requested_growth_bytes', 'peak_above_baseline_bytes',
                'returned_live_above_baseline_bytes', 'result_drop_freed_bytes', 'retained_after_result_drop_bytes', 'process_peak_rss_kib')
        mem.append(dict(case=workload, variant=variant,
                        medians={k: statistics.median(v[k] for v in found) for k in keys}, cache=found[0]['cache']))

# The references differ only in the benchmark CLI, whose original blob is pinned.
references = {label: read('references/' + label + '-reference.json') for label in ('s24', 's25')}
pins = dict(s24='d90c80b4001e43807d404454280f1511329ef952', s25='2bb9cf595f675d269406bbe783e88154ed517cb4')
cli = 'engine/search/src/bin/lab-phase-speed.rs'
for label, meta in references.items():
    assert meta['source_sha'] == pins[label] and meta['library_unchanged'] and meta['only_changed'] == cli
    assert meta['original_cli_sha256'] == '8cce269c0ceeed232a32fdba37e1b5a3ed7dbaabd97e1a589d5316b0616d19ea'
    assert meta['patched_cli_sha256'] == receipt['changed_sha256'][cli]
    patch = blob('references/' + label + '-harness.patch').decode()
    assert re.findall(r'^diff --git a/(\S+) b/\S+$', patch, re.M) == [cli]
binaries = {}
for line in blob('binaries.sha256').decode().splitlines():
    sha, path = line.split(maxsplit=1)
    binaries[Path(path.strip()).name] = sha
environment = read('cross-version/environment.json')
assert cross['timing'] and not cross['self_check'] and len(cross['results']) == 48
assert environment['timing'] and environment['sample_count'] == 1 and environment['rounds'] == 9
assert environment['binaries'] == {k: binaries[v] for k, v in dict(s24='s24-speed', s25='s25-speed', current='speed').items()}
assert cross['baseline_source'] == pins['s24'] and cross['history_source'] == pins['s25']
assert set(cross['witnesses']) == set(workloads)
for workload, evidence in cross['witnesses'].items():
    assert set(evidence) == set('abcdef')
    assert evidence['a'] == evidence['b'] == evidence['c']
    assert evidence['d'] == evidence['e'] == evidence['f']
    assert all(value == evidence['a'] for value in evidence.values())
    for version in ('s24', 's25', 'current'):
        transcript = [json.loads(line) for line in blob(f'cross-version/{workload}-{version}-stdout.jsonl').splitlines()]
        assert transcript[0] == dict(type='ready', timing=True)
        assert all(v['type'] == 'sample' and v['timing'] and v['count'] == 1 and v['ns_per_solve'] > 0 for v in transcript[1:])
        assert all(dict(witness=v['witness'], diagnostics=v['diagnostics']) == evidence['a'] for v in transcript[1:])
controls_for = {'s24-to-s25-off': 's24-aa', 's25-to-current-off': 's25-aa', 's25-to-current-history': 's25-history-aa'}
for row in cross['results']:
    assert len(row['pairs']) == 9
    assert row['ratio_median'] == statistics.median(p['ratio'] for p in row['pairs'])
    name = controls_for.get(row['comparison'], row['comparison'])
    direction(row, next(v for v in cross['results'] if v['case'] == row['case'] and v['comparison'] == name))

assert rss['processes'] == len(rss['rows']) == 120 and rss['blocks'] == 12 and not rss['timing']
assert len(rss['comparisons']) == 8
assert rss['binary_sha256'] == {k: binaries[v] for k, v in dict(old_speed='s25-speed', old_memory='s25-memory', speed='speed', memory='memory').items()}
labels = ('old-four', 'old-four-aa', 'old-five', 'current-five', 'current-six')
rss_medians = {}
for mode in ('plain', 'instrumented'):
    rss_medians[mode] = {}
    for label in labels:
        found = [v for v in rss['rows'] if v['mode'] == mode and v['label'] == label]
        assert len(found) == 12 and {v['block'] for v in found} == set(range(12))
        rss_medians[mode][label] = statistics.median(v['process_peak_rss_kib'] for v in found)
    aa = next(v for v in rss['comparisons'] if v['mode'] == mode and v['comparison'] == 'aa')
    for row in [v for v in rss['comparisons'] if v['mode'] == mode]:
        assert len(row['pairs']) == 12
        assert row['ratio_median'] == statistics.median(v['ratio'] for v in row['pairs'])
        direction(row, aa)

audit_counts = {k: sum(v[k] for v in shadow['audits']) for k in ('compiled_snapshots', 'sequence_snapshots', 'contractions', 'gap_checks')}
seconds = (datetime.fromisoformat(status['updated_at'].replace('Z', '+00:00')) - datetime.fromisoformat(status['run_started_at'].replace('Z', '+00:00'))).total_seconds()
value = dict(schema_version=1, source_sha=receipt['source_sha'], parent_sha=receipt['parent'], run_id=run,
    run_url=status['html_url'], ci_success=True, elapsed_ci_seconds=seconds, tests=tests,
    independent_lp_certificates=3405, certificate_capped=3405-oracle['converged'], distinct_fixed_games=29,
    distinct_growing_games=32, observed_exact_off_on_pairs=1537, cross_build_result_work_witnesses=56,
    intermediate_audits=audit_counts, new_callback_trace_combinations=72, inherited_history_combinations=36,
    speed_comparisons=48, eligible=len(active), censored=48-len(active), aa_controls=24, paired_rounds=9,
    profiles=profile['profiles'], speed=rows, memory=mem, memory_processes=168, portability=port,
    historical_library_references=references, cross_version=cross['results'], cross_version_environment=environment,
    sand_rss_repeat=rss['comparisons'], sand_rss_median_kib=rss_medians, sand_rss_processes=120,
    default_off=True, production_engine_unchanged=True, search_algorithm_unchanged=True,
    production_adopted=False, local_speed_measured=False, win_rate_evaluated=False, artifact_directory=folder.as_posix(),
    limitations=['One runner session; eight settings from six fixtures; no multiplicity correction',
        'Historical libraries are pinned; benchmark CLI receives the identical worker-only transport patch',
        'Cross-version ratios include compiler/layout effects, not an isolated causal measurement of each source edit',
        'Cross-version worker blocks are separate from same-binary paired blocks; do not multiply ratios across them',
        'Plain and instrumented RSS are analyzed separately; neither implies allocator cause or absence of a leak',
        'The descriptive A/A envelope is a noise screen, not a significance test',
        'Full export equality and finite oracles do not prove every future mechanic',
        'QEMU Nehalem is emulation, not physical old hardware; no win-rate evaluation'])
with (R / 'RESULTS.json').open('x', encoding='utf-8') as f:
    json.dump(value, f, ensure_ascii=False, indent=2); f.write('\n')

lines = ['# S26k 경계 키 대여 조회와 이전 버전 대조', '',
    f'[GitHub 실행]({value["run_url"]}) · `{value["source_sha"]}` · {seconds:.0f}초', '',
    '이미 등록된 public-history 경계는 `&[String]`으로 조회하고, 처음 등장한 키만 복사한다. '
    '정보 집합·키 내용·등록 순서·콜백·탐색량은 유지한다. `pipeline.borrowed_boundary`는 기본 OFF다.', '',
    f'계측 122개·일반 118개·기본 17개 테스트 통과. 32개 관측 게임에서 1,537쌍의 전체 OFF/ON JSON, '
    f'독립 BR/LP 인증 3,405건을 대조했다. 그중 {value["certificate_capped"]}건은 반복 상한 미수렴이지만 인증 수치는 일치했다. '
    '여러 설정과 부분 탐색을 포함하는 횟수이며 독립 게임 3,405개가 아니다. 56개 엔진 구성의 결과·작업량 지문이 빌드별로 일치했다.', '',
    '## 같은 바이너리의 속도', '',
    '표의 각 열은 경계 옵션만 추가한 별도 비교다. 양수는 시간 단축. 비율은 후보/기준이며 1보다 작으면 빠르다. '
    '9개 ABBA/BAAB 블록과 같은 경로 A/A를 사용한다. 서로 다른 열의 단축률을 더하지 않는다.', '',
    '| 조건 | 단독 | 이력 옵션에 추가 | 기존 5개 전체에 추가 |', '|---|---:|---:|---:|']
for workload in workloads:
    cells = []
    for variant in ('boundary', 'history-boundary', 'all-six'):
        row = next(v for v in rows if v['workload'] == workload and v['variant']['id'] == variant)
        cells.append('cap' if row['censored'] else f'{row["time_reduction_percent"]:+.2f}%')
    lines.append('| ' + workload + ' | ' + ' | '.join(cells) + ' |')
lines += ['', '| 조건 | 후보 | 기준 ms | 후보 ms | 단축률 | 비율 95% 구간 | A/A 밖 |', '|---|---|---:|---:|---:|---|---|']
for row in active:
    lo, hi = row['ratio_bootstrap95']
    lines.append(f'| {row["workload"]} | {row["variant"]["id"]} | {row["baseline_ms"]:.3f} | {row["candidate_ms"]:.3f} | '
        f'{row["time_reduction_percent"]:+.2f}% | {lo:.4f}–{hi:.4f} | {row["outside_aa_envelope"]} |')
lines += ['', '## 같은 러너의 이전 버전 대조', '',
    'S24와 S25의 라이브러리는 해당 커밋 그대로이며, 벤치마크 CLI에만 동일한 worker 통신을 추가했다. '
    '입력·풀 생성, stdin/JSON, 결과 지문 및 반환물 해제는 시간 밖이다. 세 프로세스에 순차 요청하고 각 조건 9블록을 교차 실행했다. '
    'OFF 비교는 새 코드가 컴파일된 실험 빌드의 차이를 측정한다. 기존 스칼라 빌드 비용의 측정은 아니다.', '',
    '| 조건 | 비교 | 단축률 | 비율 95% 구간 | A/A 밖 |', '|---|---|---:|---|---|']
for row in cross['results']:
    lo, hi = row['ratio_bootstrap95']
    lines.append(f'| {row["case"]} | {row["comparison"]} | {row["time_reduction_percent"]:+.2f}% | {lo:.4f}–{hi:.4f} | {row["outside_aa_envelope"]} |')
lines += ['', '## sand RSS 재현', '',
    '각 빌드 유형에서 12블록×5구성, 총 120개의 새 프로세스를 회전 순서로 실행했다. '
    '일반 빌드와 allocator 계측 빌드는 따로 해석한다. RSS는 입력과 풀 등을 포함하는 자식 프로세스 전체 peak다. '
    '단축률 대신 RSS 감소율이며, 양수면 적게 사용한다. paired ratio 중앙값과 각 구성 RSS 중앙값의 비는 서로 다를 수 있다.', '',
    '| 빌드 | S25 4개 MiB | S25 5개 MiB | 현재 5개 MiB | 현재 6개 MiB |', '|---|---:|---:|---:|---:|']
for mode, medians in rss_medians.items():
    lines.append('| ' + mode + ' | ' + ' | '.join(f'{medians[k]/1024:.2f}' for k in ('old-four', 'old-five', 'current-five', 'current-six')) + ' |')
lines += ['', '| 빌드 | 비교 | RSS 감소율 | 비율 95% 구간 | A/A 밖 |', '|---|---|---:|---|---|']
for row in rss['comparisons']:
    lo, hi = row['ratio_bootstrap95']
    lines.append(f'| {row["mode"]} | {row["comparison"]} | {row["rss_reduction_percent"]:+.2f}% | {lo:.4f}–{hi:.4f} | {row["outside_aa_envelope"]} |')
lines += ['', '## 요청 heap과 할당', '',
    '56구성×3회=168회 별도 프로세스의 중앙값. 요청 heap은 baseline 위 peak이며 속도 근거로 쓰지 않는다.', '',
    '| 조건 | 구성 | 할당 수 | 누적 요청 MiB | heap peak MiB | 반환물 MiB | RSS MiB |', '|---|---|---:|---:|---:|---:|---:|']
for row in mem:
    if row['variant'] == 'original':
        continue
    m = row['medians']
    lines.append(f'| {row["case"]} | {row["variant"]} | {m["allocations"]:,} | {m["requested_growth_bytes"]/2**20:.2f} | '
        f'{m["peak_above_baseline_bytes"]/2**20:.2f} | {m["returned_live_above_baseline_bytes"]/2**20:.2f} | {m["process_peak_rss_kib"]/1024:.2f} |')
lines += ['', '## 계측과 호환성', '',
    f'5회씩 56구성에서 19개 exclusive 단계를 확인했다. 중간 상태 대조 합계: {audit_counts}. '
    '비AVX2 QEMU Nehalem에서 CPUID를 확인한 뒤 2국면×7구성과 기존 scalar가 native 전체 결과와 일치했다. '
    '물리적인 구형 CPU의 속도 검증은 아니다.', '',
    '단일 러너 세션·유한 국면·다중 검정 미보정이라는 한계가 있다. A/A 범위는 설명용 노이즈 판단이다. '
    '생산 엔진과 기본 설정은 그대로이며 승률은 평가하지 않았다.', '',
    f'[구현·측정 계약](<{(R/"source/engine/benchmarks/s26_boundary/README.md").as_posix()}>)']
(R / 'RESULTS.md').write_text('\n'.join(lines) + '\n', encoding='utf-8')
print(json.dumps(dict(run_url=value['run_url'], seconds=seconds, capped=value['certificate_capped'],
    candidate_speed=[dict(case=v['workload'], variant=v['variant']['id'], reduction=v['time_reduction_percent'],
        interval=v['ratio_bootstrap95'], outside_aa=v['outside_aa_envelope']) for v in active if not v['variant']['id'].endswith('-aa')],
    cross_version=[v for v in cross['results'] if not v['comparison'].endswith('-aa')],
    sand_rss=[{k: v[k] for k in ('mode','comparison','rss_reduction_percent','ratio_bootstrap95','outside_aa_envelope')} for v in rss['comparisons']]), ensure_ascii=False, indent=2))
