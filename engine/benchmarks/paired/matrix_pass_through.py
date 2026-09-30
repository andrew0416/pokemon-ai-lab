"""Fresh P14 bit-exact matrix, isolated allocation and actual search proof."""
from collections import Counter
import itertools
import json
import os
from pathlib import Path
import re
import tomllib

import ci
import compact_probe as process
import run as bench

SOURCE_SHA = 'bc4fb5ef7d1aaa2400edd34931071ec6f0b64781'
MODE = 'matrix-pass-through'
PROBE = 'ci_matrix_pass_through_observer'
PROBE_PATH = 'engine/search/examples/' + PROBE + '.rs'
SOURCE_FILES = {'engine/search/Cargo.toml': 'ff37599eb689bcc90adbac8c038c46342f2226244229fa20b80c693ec4730029', 'engine/search/src/solve.rs': '0e135dbbd85dfeb4ffd589ada0e35c75b30d3c8e612651791203e7f6e3f3c42e', 'engine/search/src/solve/matrix_pass_through_observer.rs': '5118b7a1252707c525c6b99ce171bd1226dcb148d00123ea21bdf801bc0c81f4', 'engine/search/src/solve/matrix_pass_through_tests.rs': '54724eeaa178ecb1ca14ffe61dcc5c8d91c65989726de18277ef6fe2b7b321a3', 'engine/search/tests/matrix_pass_through.rs': 'd2a892b6a241ef3f29cbf14669d9186eb93cc5695a31613424f973c42f195a9c', 'engine/search/tests/p14_support.rs': '262c42b8db4b507a364dff8adfbe6b72bc2483db94f8d457379c64647822d8e1'}
PUBLIC_TEST = 'exact_off_on_matrix_search_error_and_resume_records'
LIB_PREFIX = 'solve::matrix_pass_through_tests::'
COMMON_TESTS = tuple(LIB_PREFIX + name for name in (
    'exhaustive_small_matrices_preserve_choices_bits_and_omitted_counts',
    'malformed_dimensions_keep_legacy_short_circuit_panics_and_trailing_values',
    'lazy_full_and_restricted_steps_preserve_equilibria_and_error_order',
    'eligible_owned_vectors_keep_all_three_buffers_and_capacity',
    'observer_separates_a_and_b_shortcuts_and_fallbacks',
    'isolated_allocator_proves_a_reconstruction_and_b_choice_clone_removal'))
COUNT_FIELDS = {'a_calls', 'a_passthrough', 'a_fallback', 'b_calls', 'b_passthrough', 'b_fallback'}
KINDS = {'toy':32, 'depth3':2, 'terminal':2, 'unsupported':2, 'successful-resume':1,
         'successful-replacement':1, 'nan-evaluator':4, 'fixture':7}
PUBLIC_FIELDS = {
    'toy':{'after','analysis','before','cache_stats','chance','equilibrium_bits','dominance','kind','lazy','matrix_bits','nash_value_bits','shallow_equilibrium_bits','shallow_matrix_bits','slots','stats','threads'},
    'depth3':{'after','analysis','before','kind','matrix','slots','stats','value'},
    'terminal':{'after','before','kind','refusal','result','slots','stats','value_bits'},
    'unsupported':{'after','before','kind','refusal','result','slots','stats','value_bits'},
    'successful-resume':{'after','before','kind','stats','suspension','value_bits'},
    'successful-replacement':{'after','before','kind','stats','value_bits'},
    'nan-evaluator':{'after','before','kind','lazy','slots','stats','value_bits'},
    'fixture':{'before','ending','index','instructions','kind','name','probability_bits','result','reversed','stats','suspension','value_bits'},
}


def require(value, message):
    if not value:
        raise ValueError(message)


def verify_declarations(root):
    require(re.fullmatch('[0-9a-f]{40}', SOURCE_SHA) and SOURCE_FILES,
            'P14 source is not frozen and bound')
    paths = {name: root / f'engine/{name}/Cargo.toml' for name in ('core', 'search', 'scenario')}
    features = {name: ci.read_features(path) for name, path in paths.items()}
    runtime, observer = ci.MATRIX_FEATURE, ci.MATRIX_OBSERVER
    require(features['search'].get(runtime) == [] and features['search'].get(observer) == [],
            'P14 search-only runtime or independent observer differs')
    for name in ('core', 'scenario'):
        require(runtime not in features[name] and observer not in features[name], 'P14 unexpectedly forwards runtime/observer')
    for name, path in paths.items():
        ci.reject_default_experiments(path, features[name])
    rows = [row for row in tomllib.loads(paths['search'].read_text(encoding='utf-8')).get('test', [])
            if row.get('name') == 'matrix_pass_through']
    require(len(rows) == 1 and rows[0].get('path') == 'tests/matrix_pass_through.rs'
            and not rows[0].get('required-features'), 'P14 integration test registration differs')
    for path, digest in SOURCE_FILES.items():
        require(ci.sha(root / path) == digest, 'P14 immutable source changed: ' + path)
    return {'default_off': True, 'independent_search_observer': True, 'source_file_sha256': SOURCE_FILES}


def summaries(text):
    return [tuple(map(int, row)) for row in re.findall(
        r'^test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;', text, re.M)]


def validate_counts(text, on):
    lines = re.findall(r'^P14 activation: (.+)$', text, re.M)
    require(len(lines) == 1, 'P14 public search observer evidence missing or repeated')
    value = process._json(lines[0])
    require(set(value) == COUNT_FIELDS and all(type(v) is int and v >= 0 for v in value.values()),
            'P14 observer fields malformed')
    for kind in ('a', 'b'):
        require(value[kind + '_calls'] == value[kind + '_passthrough'] + value[kind + '_fallback'],
                'P14 pass/fallback accounting differs')
    require(value['a_calls'] + value['b_calls'] > 0, 'P14 workload never reached matrix processing')
    if on:
        require(value['a_passthrough'] + value['b_passthrough'] > 0, 'P14 matrix fast path never activated')
    else:
        require(value['a_passthrough'] == value['b_passthrough'] == 0, 'P14 OFF executed the fast path')
    return value


def compare_counts(before, after):
    require(before['b_calls'] == after['b_calls'], 'P14 LazyGame full call count changed')
    require(before['a_calls'] == after['a_calls'] + after['b_passthrough'],
            'P14 ordinary calls and avoided LazyGame drop calls differ')


def variant_features(arm, compact, observer):
    flags = ci.feature_args(MODE, arm)[1].split(',')
    _, expected, _ = ci.fingerprint_expectations(MODE, arm)
    if not compact:
        flags.remove('lab-engine/' + ci.COMPACT_FEATURE)
        expected['lab-engine'][ci.COMPACT_FEATURE] = False
    if observer:
        flags.append('lab-search/' + ci.MATRIX_OBSERVER)
        expected['lab-search'][ci.MATRIX_OBSERVER] = True
    return ','.join(flags), expected


def validate_full_regressions(result):
    evidence = {}
    for arm in ('baseline', 'candidate'):
        path = result / (arm + '-build-receipt.json')
        row = json.loads(path.read_text(encoding='utf-8'))
        require(row.get('status') == 'success' and row.get('reused') is False and row.get('selection') == MODE,
                'P14 requires fresh full regressions')
        text = (result / (arm + '-build.log')).read_text(encoding='utf-8')
        passed = re.findall(r'^test (\S+) \.\.\. ok$', text, re.M)
        require(passed.count(PUBLIC_TEST) == 1, 'P14 public test missing from full release regressions')
        evidence[arm] = {'receipt_sha256': ci.sha(path), 'log_sha256': ci.sha(result / (arm + '-build.log'))}
    return evidence


def validate_tests(text, expected, *, lib=False, on=False):
    passed = re.findall(r'^test (\S+) \.\.\. ok$', text, re.M)
    require(all(passed.count(name) == 1 for name in expected), 'P14 required named test skipped or duplicated')
    counts = summaries(text)
    require(counts and counts[-1][0] >= len(expected) and counts[-1][1:] == (0,0,0,0), 'P14 test suite incomplete')
    if not lib:
        require(passed == [PUBLIC_TEST] and counts == [(1,0,0,0,0)], 'P14 public target must run exactly one test')
    else:
        require(len(counts) == 2 and counts[0][0:4] == (1,0,0,0), 'P14 allocator isolated child did not execute')
        require(re.findall(r'^P14 MATRIX CASES (\d+)$', text, re.M) == ['1378'], 'P14 matrix corpus did not execute')
        # libtest --nocapture may prefix the child's marker with its test name.
        # Accept exactly that prefix or a standalone marker, never arbitrary text.
        child = re.escape(LIB_PREFIX + 'isolated_allocator_proves_a_reconstruction_and_b_choice_clone_removal')
        alloc = re.findall(r'^(?:test ' + child + r' \.\.\. )?P14 ALLOC candidate=(true|false) a_reference=(\d+) a_actual=(\d+) b_reference=(\d+) b_actual=(\d+)$', text, re.M)
        require(len(alloc) == 1 and alloc[0][0] == str(on).lower(), 'P14 allocation proof missing or repeated')
        a_ref, a_actual, b_ref, b_actual = map(int, alloc[0][1:])
        require(a_ref > 0 and b_ref >= 2, 'P14 allocation reference was not exercised')
        require((a_actual, b_actual) == ((0,0) if on else (a_ref,b_ref)), 'P14 actual allocations did not match runtime')
        if on:
            require(b_ref == 2, 'P14b reference must isolate choice clones after P14a')
    return {'passed':len(passed), 'required_named_tests':list(expected), 'summaries':counts}


def validate_records(path, *, private=False):
    raw = path.read_bytes()
    require(raw.endswith(b'\n') and b'\r' not in raw, 'P14 complete LF JSONL required')
    rows = [process._json(line) for line in raw.splitlines()]
    require(all(isinstance(row, dict) for row in rows), 'P14 record is not an object')
    if private:
        expected = []
        pool = [0,0x80000000,0x7f800000,0xff800000,1,0x80000001,0x3f800000,0xbf800000]
        nan = [0x7fc00001,0x7fa00023,0xffc00456]
        for n, m in itertools.product(range(4), range(4)):
            for mask in range(1 << (n*m)):
                values = [pool[i % len(pool)] if mask & (1 << i) == 0 else nan[i % len(nan)] for i in range(n*m)]
                for slots in (1,2):
                    expected.append((slots,n,m,values))
        require(len(rows) == len(expected) == 1378, 'P14 matrix corpus size differs')
        for row, identity in zip(rows, expected):
            require(set(row) == {'slots','rows','cols','input','result'}, 'P14 matrix record fields differ')
            require((row['slots'],row['rows'],row['cols'],row['input']) == identity, 'P14 matrix corpus input/order differs')
            require(set(row['result']) == {'ok'}, 'P14 well-shaped matrix unexpectedly panicked')
            result = row['result']['ok']
            require(set(result) == {'ours','theirs','values','omitted_ours','omitted_theirs'}, 'P14 matrix result fields differ')
            require(all(isinstance(result[k], str) for k in ('ours','theirs')), 'P14 choice order evidence missing')
            require(all(type(v) is int and 0 <= v < 2**32 for v in result['values']), 'P14 cell bits malformed')
            require(all(type(result[k]) is int and result[k] >= 0 for k in ('omitted_ours','omitted_theirs')), 'P14 omitted counts missing')
    else:
        require(Counter(row.get('kind') for row in rows) == KINDS, 'P14 public case counts differ')
        combos = {(r['slots'],r['chance'],r['threads'],r['dominance'],r['lazy']) for r in rows if r['kind']=='toy'}
        require(combos == set(itertools.product((1,2),('Expect','Worst'),(1,2),(False,True),(False,True))), 'P14 bounded search axes differ')
        nan_cases = {(r['slots'],r['lazy']) for r in rows if r['kind']=='nan-evaluator'}
        require(nan_cases == set(itertools.product((1,2),(False,True))), 'P14 NaN evaluator coverage differs')
        fixtures = [(r.get('name'),r.get('index')) for r in rows if r['kind']=='fixture']
        require(fixtures == [('aa-power-construct',i) for i in range(4)] + [('ability-change-fails',0),('eject-button-uturn',0),('eject-button-uturn',1)], 'P14 turn fixture ordering differs')
        for row in rows:
            require(set(row) == PUBLIC_FIELDS[row['kind']], 'P14 complete public record fields differ')
            for key in ('before','after','ending','reversed'):
                if key in row:
                    state = row[key]
                    require(set(state) == {'debug','full_hash','position_hash'} and isinstance(state['debug'],str) and bool(state['debug'])
                            and all(type(state[k]) is int and 0 <= state[k] < 2**64 for k in ('full_hash','position_hash')), 'P14 full State Debug/hash missing')
            require(len(row['stats']) == 7 and all(type(v) is int and v >= 0 for v in row['stats']), 'P14 integer statistics missing')
            if 'after' in row:
                require(row['after'] == row['before'], 'P14 search restoration failed')
            if row['kind']=='fixture':
                require(row['reversed'] == row['before'] and isinstance(row['instructions'],str) and isinstance(row['suspension'],str), 'P14 instruction/suspension/rollback evidence missing')
            if row['kind']=='nan-evaluator':
                v = row['value_bits']
                require(type(v) is int and 0 <= v < 2**32 and v & 0x7f800000 == 0x7f800000 and v & 0x7fffff != 0, 'P14 NaN sentinel bits missing')
    return {'records':len(rows), 'sha256':ci.sha(path), 'bytes':len(raw)}


def validate(workspace):
    workspace = Path(workspace).resolve()
    result = workspace / 'ci-results'
    out = result / 'matrix-pass-through-validation'
    out.mkdir(exist_ok=False)
    receipt = {'status': 'running', 'selection': MODE, 'cached_results_reused': False,
               'performance_measurement': False, 'common_runtime': 'old4+P8d; E/F/P11/P12/P13 OFF; dense disables P10 only',
               'bounded_profile': 'debug opt0', 'public_activation_profile': 'release opt3',
               'commands': [], 'variants': {}, 'activation': {}}

    def save():
        (out / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n', encoding='utf-8')

    def text(stem):
        return stem.with_suffix('.stdout').read_text(encoding='utf-8') + '\n' + stem.with_suffix('.stderr').read_text(encoding='utf-8')

    def execute(argv, env, stem):
        return process._run(argv, workspace / 'candidate/engine', env, stem,
                            ci.COMMAND_TIMEOUT_SECONDS, receipt['commands'], save)

    def environment(label):
        env = process.environment(workspace / ('target-' + label))
        for name in ('LAB_P14_RECORDS', 'LAB_P14_UNIT_RECORDS', 'LAB_P14_ALLOC_CHILD', 'LAB_ENGINE_STATS'):
            env.pop(name, None)
        env.update(RAYON_NUM_THREADS='1', RUST_MIN_STACK='16777216', CARGO_PROFILE_DEV_OPT_LEVEL='0',
                   CARGO_PROFILE_TEST_OPT_LEVEL='0', CARGO_PROFILE_DEV_DEBUG='0', CARGO_PROFILE_TEST_DEBUG='0')
        return env

    save()
    try:
        request = json.loads((result / 'request.json').read_text(encoding='utf-8'))
        require(request['candidate_feature'] == MODE and request['baseline_sha'] == request['candidate_sha'] == SOURCE_SHA,
                'P14 fixed same-source common5 request required')
        receipt['declarations'] = verify_declarations(workspace / 'candidate')
        require(COMMON_TESTS and 'UNBOUND' not in PUBLIC_TEST, 'P14 named tests are not frozen')
        receipt['full_regressions'] = validate_full_regressions(result)
        labels = ['p14-' + storage + '-' + arm + '-' + kind
                  for storage in ('dense', 'compact') for arm in ('baseline', 'candidate') for kind in ('plain', 'observer')]
        require(all(not os.path.lexists(workspace / ('target-' + label)) for label in labels), 'P14 proof targets must be fresh')
        records, private_records = [], []
        for storage, arm, kind in itertools.product(('dense', 'compact'), ('baseline', 'candidate'), ('plain', 'observer')):
            compact, observer, on = storage == 'compact', kind == 'observer', arm == 'candidate'
            label = 'p14-' + storage + '-' + arm + '-' + kind
            env = environment(label)
            flags, expected = variant_features(arm, compact, observer)
            item = {'features': flags}
            receipt['variants'][label] = item
            if observer:
                env['LAB_P14_UNIT_RECORDS'] = str(out / (label + '-private.jsonl'))
                stem = out / (label + '-lib')
                execute(['cargo', 'test', '--locked', '-p', 'lab-search', '--lib', '--features', flags,
                         '--', '--show-output', '--test-threads=1'], env, stem)
                item['named_tests'] = validate_tests(text(stem), COMMON_TESTS, lib=True, on=on)
                private_path = Path(env['LAB_P14_UNIT_RECORDS'])
                item['private'] = validate_records(private_path, private=True)
                private_records.append(private_path)
            env['LAB_P14_RECORDS'] = str(out / (label + '.jsonl'))
            stem = out / (label + '-public')
            execute(['cargo', 'test', '--locked', '-p', 'lab-search', '--test', 'matrix_pass_through',
                     '--features', flags, '--', '--show-output', '--test-threads=1'], env, stem)
            item['public_test'] = validate_tests(text(stem), (PUBLIC_TEST,))
            activation_lines = re.findall(r'^P14_PUBLIC_ACTIVATION (.+)$', text(stem), re.M)
            require(len(activation_lines) == int(observer), 'P14 bounded observer missing or present in plain build')
            if observer:
                bounded = validate_counts('P14 activation: ' + activation_lines[0], on)
                require(bounded['a_calls'] > 0 and bounded['b_calls'] > 0, 'P14 bounded coverage missed a or b')
                if on:
                    require(all(bounded[k] > 0 for k in ('a_passthrough','a_fallback','b_passthrough','b_fallback')), 'P14 bounded coverage missed pass/fallback')
                item['bounded_activation'] = bounded
            record_path = Path(env['LAB_P14_RECORDS'])
            item['public'] = validate_records(record_path)
            records.append(record_path)
            packages = {'lab-engine': ('lib-lab_engine.json',), 'lab-search': (
                'lib-lab_search.json', 'test-integration-test-matrix_pass_through.json') + (('test-lib-lab_search.json',) if observer else ())}
            item['compiler_feature_evidence'] = ci.preserve_expected_fingerprints(workspace, label, packages, expected, True, profile='debug')
            if compact and observer:
                require(ci.sha(workspace / 'candidate' / PROBE_PATH) == ci.sha(Path(__file__).with_name('matrix_pass_through_probe.rs')),
                        'P14 observer wrapper injection differs')
                execute(['cargo', 'build', '--locked', '--release', '-p', 'lab-search', '--example', PROBE, '--features', flags],
                        env, out / (label + '-probe-build'))
                item['probe_feature_evidence'] = ci.preserve_expected_fingerprints(workspace, label, {
                    'lab-engine': ('lib-lab_engine.json',), 'lab-search': ('lib-lab_search.json', 'example-' + PROBE + '.json')}, expected, True)
                binary = workspace / ('target-' + label) / 'release/examples' / PROBE
                require(binary.is_file() and not binary.is_symlink(), 'P14 observer binary missing')
                item['probe_binary_sha256'] = ci.sha(binary)
                cases, _ = bench.load_cases('narrow', root=workspace / 'controller')
                require([case['name'] for case in cases] == ['coaching', 'sand'], 'P14 fixed workload selection changed')
                receipt['activation'][arm] = {}
                for case in cases:
                    stem = out / (label + '-' + case['name'])
                    execute([str(binary), str(workspace / 'controller' / case['scenario']), '1', case['position']], env, stem)
                    output = stem.with_suffix('.stdout')
                    bench.validate_output(bench.strict_json(output.read_text(encoding='utf-8')))
                    counts = validate_counts(stem.with_suffix('.stderr').read_text(encoding='utf-8'), on)
                    receipt['activation'][arm][case['name']] = {'counts': counts, 'output_sha256': ci.sha(output),
                                                              'output_bytes': output.stat().st_size, 'output_file': output.name}
            save()
        require(all(path.read_bytes() == records[0].read_bytes() for path in records), 'P14 OFF/ON/observer/storage records differ')
        require(all(path.read_bytes() == private_records[0].read_bytes() for path in private_records), 'P14 matrix bits/choice/omitted records differ')
        for storage in ('dense','compact'):
            compare_counts(receipt['variants']['p14-'+storage+'-baseline-observer']['bounded_activation'],
                           receipt['variants']['p14-'+storage+'-candidate-observer']['bounded_activation'])
        for case in ('coaching', 'sand'):
            before, after = receipt['activation']['baseline'][case], receipt['activation']['candidate'][case]
            require((out / before['output_file']).read_bytes() == (out / after['output_file']).read_bytes(), 'P14 real search output/work differs')
            compare_counts(before['counts'], after['counts'])
        receipt.update(status='success', source_sha=SOURCE_SHA, complete_jsonl_byte_equal=True, private_jsonl_byte_equal=True,
                       actual_public_search_activated=True)
    except Exception as error:
        receipt.update(status='failed', error=f'{type(error).__name__}: {error}')
        raise
    finally:
        save()
    return receipt
