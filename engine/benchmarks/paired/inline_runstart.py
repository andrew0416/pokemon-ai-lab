"""Fresh P11 correctness/allocation proof on common5; never reuse local common6 outputs."""
import json
import os
from pathlib import Path
import re
import tomllib

import ci
import compact_probe as process

CORE_TESTS = tuple('turn::battle::inline_runstart_tests::' + name for name in (
    'runstart_lengths_order_clone_and_ownership_are_preserved',
    'newcomer_vec_growth_and_replay_do_not_mutate_runstart',
    'replay_restores_inline_and_spilled_entries_exactly'))
SCENARIO_TEST = 'bounded_runstart_records_preserve_full_logical_behavior'
ALLOCATION_TEST = 'runstart_capture_and_clone_remove_only_inline_allocations'
LENGTHS = (0, 1, 2, 4, 5, 12, 31)
SOURCE_TESTS = {
    'engine/core/src/turn/battle/inline_runstart_tests.rs': '847ac8d832986b43723b059f4051a87280a57b8129ee83a30566949c68bcf9e5',
    'engine/core/tests/inline_runstart_allocations.rs': '90d6a4d6041ac1b9fb7494970481c85d0e5292e83c49acf1a3c6dda439b16d12',
    'engine/scenario/tests/inline_runstart.rs': '0e52749cee91e45e3b6e6d0e4f101d5fb668028c8cabe4ca863460d1ea192fcb',
}
CASES = {'single-hit', 'f-trick-room-speed-wrap', 'f-trick-room-raw-speed',
         'f-speed-snapshot-eject-pack', 'electro-ball-gyro-ball', 'o44-quickfeet',
         'stance-change-raw-speed', 'speed-swap-switch', 'aa-quick-draw', 'double-hit',
         'after-you', 'quash', 'aa-power-construct-faint', 'uturn-switch', 'ae-singles-hit'}


def require(value, message):
    if not value:
        raise ValueError(message)


def verify_declarations(root):
    core_path = root/'engine/core/Cargo.toml'
    scenario_path = root/'engine/scenario/Cargo.toml'
    search_path = root/'engine/search/Cargo.toml'
    core, scenario, search = (ci.read_features(path) for path in (core_path, scenario_path, search_path))
    require(core.get(ci.INLINE_FEATURE) == [] and core.get(ci.INLINE_OBSERVER) == [ci.INLINE_FEATURE],
            'P11 core runtime/observer declarations differ')
    require(scenario.get(ci.INLINE_OBSERVER) == ['lab-engine/' + ci.INLINE_OBSERVER],
            'P11 scenario observer forwarding differs')
    forbidden = {ci.INLINE_FEATURE, ci.INLINE_OBSERVER}
    require(not forbidden.intersection(search) and not any(
        value.rsplit('/', 1)[-1] in forbidden for values in search.values() for value in values),
        'P11 must not add search feature forwarding')
    for path, features in ((core_path, core), (scenario_path, scenario), (search_path, search)):
        ci.reject_default_experiments(path, features)
    for path, name, required in ((core_path, 'inline_runstart_allocations', [ci.INLINE_OBSERVER]),
                                 (scenario_path, 'inline_runstart', None)):
        data = tomllib.loads(path.read_text(encoding='utf-8'))
        rows = [row for row in data.get('test', []) if row.get('name') == name]
        require(len(rows) == 1 and rows[0].get('path') == 'tests/' + name + '.rs'
                and rows[0].get('required-features') == required, 'P11 exact test target registration differs')
    for relative, digest in SOURCE_TESTS.items():
        require(ci.sha(root/relative) == digest, 'P11 frozen source test changed: ' + relative)
    return {'runtime_default_off': True, 'observer_separate': True, 'source_test_sha256': SOURCE_TESTS}


def summaries(text):
    return [tuple(map(int, row)) for row in re.findall(
        r'^test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;',
        text, re.M)]


def named_tests(text, expected, *, filtered=False):
    passed = re.findall(r'^test (\S+) \.\.\. ok$', text, re.M)
    require(sorted(passed) == sorted(expected), 'P11 exact named tests missing, repeated or skipped')
    counts = summaries(text)
    require(len(counts) == 1 and counts[0][:4] == (len(expected), 0, 0, 0)
            and (filtered or counts[0][4] == 0), 'P11 named test summary differs')


def validate_full_regressions(result):
    receipts = {}
    for arm in ('baseline', 'candidate'):
        row = json.loads((result/(arm + '-build-receipt.json')).read_text(encoding='utf-8'))
        require(row.get('status') == 'success' and row.get('reused') is False
                and row.get('selection') == ci.INLINE_MODE, 'P11 requires fresh full regression arms')
        log = result/(arm + '-build.log')
        passed = re.findall(r'^test (\S+) \.\.\. ok$', log.read_text(encoding='utf-8'), re.M)
        require(all(passed.count(name) == 1 for name in (*CORE_TESTS, SCENARIO_TEST)),
                'P11 OFF/ON core and scenario tests must execute in full regressions')
        receipts[arm] = {'passed_core_tests': list(CORE_TESTS), 'scenario_test': SCENARIO_TEST,
                         'log_sha256': ci.sha(log), 'build_receipt_sha256': ci.sha(result/(arm+'-build-receipt.json'))}
    return receipts


def validate_allocator(text):
    matches = re.findall(r'P11 allocation proof: len=(\d+) original=(\d+) capture=(\d+) clone=(\d+)', text)
    actual = [tuple(map(int, row)) for row in matches]
    expected = [(n, int(n > 0), int(n > 4), int(n > 4)) for n in LENGTHS]
    require(actual == expected, 'P11 isolated allocator length/count proof differs')
    require(text.count('test ' + ALLOCATION_TEST + ' ...') == 2
            and summaries(text) == [(1, 0, 0, 0, 0), (1, 0, 0, 0, 0)],
            'P11 allocator must run its exact parent and isolated child')
    return {'parent_tests': 1, 'isolated_child_tests': 1, 'lengths': list(LENGTHS), 'allocation_counts': actual}


def validate_records(path):
    raw = path.read_bytes()
    require(raw.endswith(b'\n') and b'\r' not in raw, 'P11 records must be complete LF JSONL')
    rows = [process._json(line) for line in raw.splitlines()]
    require(bool(rows) and all(isinstance(row, dict) for row in rows), 'P11 records missing')
    kinds = [row.get('kind') for row in rows]
    require(kinds[-1] == 'coverage' and kinds.count('coverage') == 1
            and set(kinds) == {'enumeration', 'sample', 'coverage'}, 'P11 record kinds/completion differ')
    enums = [row for row in rows if row['kind'] == 'enumeration']
    samples = [row for row in rows if row['kind'] == 'sample']
    require({row.get('case') for row in enums} == CASES and {row.get('slots') for row in enums} == {1, 2}
            and {row.get('factored') for row in enums} == {False, True}
            and {row.get('rolls') for row in enums} == {'Median', 'Full'}, 'P11 fixed fixture/configuration coverage differs')
    require({row.get('seed') for row in samples} == {0, 7, 42}, 'P11 sample seeds changed')
    fields = {'probability_bits', 'instructions', 'instructions_debug', 'suspension',
              'suspension_debug', 'end_state', 'end_state_debug', 'end_position_hash'}
    outcomes = 0
    errors = successes = 0
    for row in enums:
        require(all(key in row for key in ('before_state', 'before_position_hash', 'before_state_debug',
                                          'before_party_order', 'decision', 'position_probability_bits')),
                'P11 full before state/order/hash missing')
        result = row.get('result', {})
        if set(result) == {'ok'}:
            successes += 1
            require(isinstance(result['ok'], list) and bool(result['ok']), 'P11 empty success')
            outcomes += len(result['ok'])
            for ending in result['ok']:
                require(set(ending) == {'outcome', 'party_order'} and set(ending['outcome']) == fields,
                        'P11 complete outcome/order/instruction proof missing')
        else:
            errors += 1
            require(set(result) == {'error_debug', 'error_display'}, 'P11 error evidence missing')
    for row in samples:
        require(row.get('sample_count') == 4 and isinstance(row.get('outcomes'), list)
                and bool(row['outcomes']) and all(set(ending) == fields for ending in row['outcomes']),
                'P11 sample full output missing')
    cover = rows[-1]
    require(set(cover) == {'kind','positions','outcomes','errors','successes','suspended','resumed','sample_calls'}
            and all(type(value) is int and value >= 0 for key,value in cover.items() if key != 'kind'),
            'P11 coverage counts malformed')
    require((cover['positions'], cover['outcomes'], cover['errors'], cover['successes'], cover['sample_calls'])
            == (len(enums), outcomes, errors, successes, len(samples))
            and successes > errors > 0 and cover['suspended'] > 0 and cover['resumed'] > 0
            and len(samples) >= 9, 'P11 actual coverage is incomplete or inconsistent')
    return {'records':len(rows),'bytes':len(raw),'sha256':ci.sha(path),'coverage':cover}


def validate_activation(text, expected_records):
    rows = re.findall(r'P11 activation: Counts \{ captures: (\d+), inline_snapshots: (\d+), '
                      r'spilled_snapshots: (\d+), empty_snapshots: (\d+), snapshot_entries: (\d+) \}',text)
    require(len(rows) == 1, 'P11 actual scenario observer activation missing/repeated')
    capture, inline, spill, empty, entries = map(int, rows[0])
    require(capture == inline + spill and inline > 0 and spill == 0 and 0 <= empty <= inline
            and entries > 0, 'P11 scenario observer did not activate the inline path')
    require(re.findall(r'P11 deterministic records: (\d+)',text) == [str(expected_records)],
            'P11 observer record count differs')
    return dict(captures=capture, inline_snapshots=inline, spilled_snapshots=spill,
                empty_snapshots=empty, snapshot_entries=entries)


def validate(workspace):
    workspace = Path(workspace).resolve()
    result = workspace/'ci-results'
    out = result/'inline-runstart-validation'
    out.mkdir(exist_ok=False)
    receipt = {'status':'running','selection':ci.INLINE_MODE,'performance_measurement':False,
               'cached_results_reused':False,'common_runtime':'P8g/P8c/P9/P10/P8d; P8e/P8f OFF',
               'commands':[],'scenario':{}}
    def save():
        (out/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n',encoding='utf-8')
    def text(stem):
        return stem.with_suffix('.stdout').read_text(encoding='utf-8') + '\n' + stem.with_suffix('.stderr').read_text(encoding='utf-8')
    def env_for(label):
        env = process.environment(workspace/('target-'+label))
        for name in ('LAB_P11_RECORDS','LAB_P11_ALLOCATION_CHILD','LAB_ENGINE_STATS'):
            env.pop(name,None)
        env['RAYON_NUM_THREADS']='1'
        return env
    def execute(argv, env, stem):
        return process._run(argv,workspace/'candidate/engine',env,stem,ci.COMMAND_TIMEOUT_SECONDS,receipt['commands'],save)
    save()
    try:
        request=json.loads((result/'request.json').read_text(encoding='utf-8'))
        require(request['candidate_feature']==ci.INLINE_MODE and request['baseline_sha']==request['candidate_sha'],
                'P11 fresh proof requires same-source inline-runstart request')
        receipt['full_regressions']=validate_full_regressions(result)
        receipt['declarations']=verify_declarations(workspace/'candidate')
        labels=['inline-runstart-core-observer', *('inline-runstart-records-'+arm for arm in ('baseline','candidate','observer'))]
        require(all(not os.path.lexists(workspace/('target-'+label)) for label in labels), 'P11 proof targets must be fresh')
        _,expected,_=ci.fingerprint_expectations(ci.INLINE_MODE,'candidate')
        observer_core=dict(expected['lab-engine'], **{ci.INLINE_OBSERVER:True})
        flags=','.join('lab-engine/'+name for name,active in observer_core.items() if active)
        label=labels[0]; env=env_for(label)
        core_stem=out/'observer-core'
        execute(['cargo','test','--locked','--release','-p','lab-engine','--lib','inline_runstart',
                 '--features',flags,'--','--test-threads=1'],env,core_stem)
        named_tests(text(core_stem),CORE_TESTS,filtered=True)
        alloc_stem=out/'allocator'
        execute(['cargo','test','--locked','--release','-p','lab-engine','--test','inline_runstart_allocations',
                 '--features',flags,'--','--show-output','--test-threads=1'],env,alloc_stem)
        receipt['allocator']=validate_allocator(text(alloc_stem))
        receipt['observer_core_features']=ci.preserve_expected_fingerprints(workspace,label,
            {'lab-engine':('lib-lab_engine.json','test-lib-lab_engine.json','test-integration-test-inline_runstart_allocations.json')},
            {'lab-engine':observer_core},True)
        for arm in ('baseline','candidate','observer'):
            label='inline-runstart-records-'+arm;env=env_for(label)
            record_path=out/(arm+'.jsonl');env['LAB_P11_RECORDS']=str(record_path)
            _,features,_=ci.fingerprint_expectations(ci.INLINE_MODE,'baseline' if arm=='baseline' else 'candidate')
            core=features['lab-engine'];scenario={name:False for name in ci.ALL_EXPERIMENT_FEATURES}
            if arm=='observer': core[ci.INLINE_OBSERVER]=True;scenario[ci.INLINE_OBSERVER]=True
            flags=','.join('lab-engine/'+name for name,active in core.items() if active and name!=ci.INLINE_OBSERVER)
            if arm=='observer':flags+=',lab-scenario/'+ci.INLINE_OBSERVER
            stem=out/('scenario-'+arm)
            execute(['cargo','test','--locked','--release','-p','lab-scenario','--test','inline_runstart',
                     '--features',flags,SCENARIO_TEST,'--','--exact','--show-output','--test-threads=1'],env,stem)
            named_tests(text(stem),(SCENARIO_TEST,))
            evidence=ci.preserve_expected_fingerprints(workspace,label,
                {'lab-engine':('lib-lab_engine.json',),'lab-scenario':('lib-lab_scenario.json','test-integration-test-inline_runstart.json')},
                {'lab-engine':core,'lab-scenario':scenario},True)
            record=validate_records(record_path)
            receipt['scenario'][arm]={'output':record,'compiler_feature_evidence':evidence}
            if arm=='observer':receipt['activation']=validate_activation(text(stem),record['records'])
            else:require('P11 activation:' not in text(stem),'P11 observer leaked into OFF/ON proof')
            save()
        raw=[(out/(arm+'.jsonl')).read_bytes() for arm in ('baseline','candidate','observer')]
        require(raw[0]==raw[1]==raw[2],'P11 OFF/ON/observer full records must be byte-identical')
        receipt.update(status='success',complete_jsonl_byte_equal=True,source_sha=request['candidate_sha'])
    except Exception as error:
        receipt.update(status='failed',error=f'{type(error).__name__}: {error}')
        raise
    finally:save()
    return receipt
