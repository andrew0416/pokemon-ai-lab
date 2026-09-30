"""Fresh R1/P14 experiment; immutable older validators remain unchanged."""
import itertools
import json
import os
from pathlib import Path
import re
import tomllib
import ci
import compact_probe as process
import run as bench
import matrix_pass_through as previous
import borrowed_child_keys as borrowed

SOURCE_SHA='5097e5c1a91f94f00dff0edfe0a08381f591741f'
SOURCE_FILES={'engine/search/Cargo.toml': '228a4c976cc881c8c78e939bbb68ba35585b4db2db2ff45337683eb3c50d34b9', 'engine/search/src/solve.rs': '0fe18477ad494c313bc144f35631dfdab22adbc47c71c172288ce6f6f308a90e', 'engine/search/src/solve/matrix_pass_through_observer.rs': '5118b7a1252707c525c6b99ce171bd1226dcb148d00123ea21bdf801bc0c81f4', 'engine/search/src/solve/matrix_pass_through_tests.rs': '54724eeaa178ecb1ca14ffe61dcc5c8d91c65989726de18277ef6fe2b7b321a3', 'engine/search/tests/matrix_pass_through.rs': 'd2a892b6a241ef3f29cbf14669d9186eb93cc5695a31613424f973c42f195a9c', 'engine/search/tests/p14_support.rs': '262c42b8db4b507a364dff8adfbe6b72bc2483db94f8d457379c64647822d8e1'}
MODE='r1-matrix-pass-through'
PROBE='ci_r1_matrix_pass_through_observer'
PROBE_PATH='engine/search/examples/'+PROBE+'.rs'
PUBLIC_TEST=previous.PUBLIC_TEST
COMMON_TESTS=previous.COMMON_TESTS
require=previous.require
validate_tests=previous.validate_tests
validate_records=previous.validate_records
PROOF_VARIANTS=tuple((storage,arm,kind) for storage,kind in
                    (('dense','observer'),('compact','observer'),('compact','plain'))
                    for arm in ('baseline','candidate'))

def validate_counts(text,on):
    lines=re.findall(r'^R1_P14 activation: (.+)$',text,re.M)
    require(len(lines)==1,'R1/P14 joint observer missing or repeated')
    value=process._json(lines[0])
    require(isinstance(value,dict) and set(value)=={'matrix','borrowed'},'R1/P14 observer sections differ')
    previous.validate_counts('P14 activation: '+json.dumps(value['matrix']),on)
    borrowed.validate_counts('P13 activation: '+json.dumps(value['borrowed']),True)
    return value

def compare_counts(before,after):
    previous.compare_counts(before['matrix'],after['matrix'])
    require(before['borrowed']==after['borrowed'],'R1/P14 changed borrowed-key job/query work')

def actual_features(arm):
    flags,expected=variant_features(arm,True,True)
    flags+=',lab-search/'+ci.BORROWED_OBSERVER
    expected['lab-search'][ci.BORROWED_OBSERVER]=True
    return flags,expected

def borrowed_proof_features():
    # The P13 and P14 tests each own a global allocator. Source test-only cfg
    # isolates their libtest executables; production and example code is unchanged.
    flags,expected=variant_features('candidate',True,True)
    flags+=',lab-search/'+ci.BORROWED_OBSERVER
    expected['lab-search'][ci.BORROWED_OBSERVER]=True
    return flags,expected

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
    require(features['core'].get(ci.BORROWED_FEATURE)==[] and ci.BORROWED_OBSERVER not in features['core']
            and features['search'].get(ci.BORROWED_FEATURE)==['lab-engine/'+ci.BORROWED_FEATURE]
            and features['search'].get(ci.BORROWED_OBSERVER)==[], 'R1 P13 declaration changed')
    for name, path in paths.items():
        ci.reject_default_experiments(path, features[name])
    rows = [row for row in tomllib.loads(paths['search'].read_text(encoding='utf-8')).get('test', [])
            if row.get('name') == 'matrix_pass_through']
    require(len(rows) == 1 and rows[0].get('path') == 'tests/matrix_pass_through.rs'
            and not rows[0].get('required-features'), 'P14 integration test registration differs')
    for path, digest in SOURCE_FILES.items():
        require(ci.sha(root / path) == digest, 'P14 immutable source changed: ' + path)
    return {'default_off': True, 'independent_search_observer': True, 'source_file_sha256': SOURCE_FILES}


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
        require(passed.count(borrowed.PUBLIC_TEST)==1 and all(passed.count(name)==1 for name in borrowed.ON_TESTS[2:]),
                'R1 P13 public/TT regression coverage missing')
        evidence[arm] = {'receipt_sha256': ci.sha(path), 'log_sha256': ci.sha(result / (arm + '-build.log'))}
    return evidence


def validate(workspace):
    workspace = Path(workspace).resolve()
    result = workspace / 'ci-results'
    out = result / 'r1-matrix-pass-through-validation'
    out.mkdir(exist_ok=False)
    receipt = {'status': 'running', 'selection': MODE, 'cached_results_reused': False,
               'performance_measurement': False, 'common_runtime': 'R1 old4+P8d+P13; E/F/P11/P12/P15 OFF; dense disables P10 only',
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
        for name in ('LAB_P14_RECORDS', 'LAB_P14_UNIT_RECORDS', 'LAB_P14_ALLOC_CHILD',
                     'LAB_P13_RECORDS', 'LAB_P13_UNIT_RECORDS', 'LAB_P13_ALLOC_CHILD', 'LAB_ENGINE_STATS'):
            env.pop(name, None)
        env.update(RAYON_NUM_THREADS='1', RUST_MIN_STACK='16777216', CARGO_PROFILE_DEV_OPT_LEVEL='0',
                   CARGO_PROFILE_TEST_OPT_LEVEL='0', CARGO_PROFILE_DEV_DEBUG='0', CARGO_PROFILE_TEST_DEBUG='0')
        return env

    save()
    try:
        request = json.loads((result / 'request.json').read_text(encoding='utf-8'))
        require(request['candidate_feature'] == MODE and request['baseline_sha'] == request['candidate_sha'] == SOURCE_SHA,
                'R1/P14 fixed same-source request required')
        receipt['declarations'] = verify_declarations(workspace / 'candidate')
        require(COMMON_TESTS and 'UNBOUND' not in PUBLIC_TEST, 'P14 named tests are not frozen')
        receipt['full_regressions'] = validate_full_regressions(result)
        labels = ['r1-p14-' + storage + '-' + arm + '-' + kind
                  for storage, arm, kind in PROOF_VARIANTS]
        labels.append('r1-p14-borrowed-proof')
        require(all(not os.path.lexists(workspace / ('target-' + label)) for label in labels), 'P14 proof targets must be fresh')
        records, private_records = [], []
        for storage, arm, kind in PROOF_VARIANTS:
            compact, observer, on = storage == 'compact', kind == 'observer', arm == 'candidate'
            label = 'r1-p14-' + storage + '-' + arm + '-' + kind
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
                bounded = previous.validate_counts('P14 activation: ' + activation_lines[0], on)
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
                require(ci.sha(workspace / 'candidate' / PROBE_PATH) == ci.sha(Path(__file__).with_name('r1_matrix_pass_through_probe.rs')),
                        'P14 observer wrapper injection differs')
                actual_flags, actual_expected = actual_features(arm)
                execute(['cargo', 'build', '--locked', '--release', '-p', 'lab-search', '--example', PROBE, '--features', actual_flags],
                        env, out / (label + '-probe-build'))
                item['probe_feature_evidence'] = ci.preserve_expected_fingerprints(workspace, label, {
                    'lab-engine': ('lib-lab_engine.json',), 'lab-search': ('lib-lab_search.json', 'example-' + PROBE + '.json')}, actual_expected, True)
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
        # One compact candidate debug libtest preserves the established P13
        # allocation/collision/error tests without mixing its allocator with P14's.
        label='r1-p14-borrowed-proof';env=environment(label)
        env['LAB_P13_UNIT_RECORDS']=str(out/(label+'.jsonl'))
        flags,expected=borrowed_proof_features();stem=out/(label+'-lib')
        execute(['cargo','test','--locked','-p','lab-search','--lib','--features',flags,
                 '--','--show-output','--test-threads=1'],env,stem)
        receipt['borrowed_proof']={
            'features':flags,'tests':borrowed.validate_tests(text(stem),borrowed.COMMON_TESTS+borrowed.ON_TESTS,lib=True,on=True,compact=True),
            'records':borrowed.validate_records(Path(env['LAB_P13_UNIT_RECORDS']),private=True),
            'compiler_feature_evidence':ci.preserve_expected_fingerprints(workspace,label,{
                # --lib compiles the search test crate directly; a normal
                # lib-lab_search fingerprint is not guaranteed in this target.
                'lab-engine':('lib-lab_engine.json',),'lab-search':('test-lib-lab_search.json',)},expected,True,profile='debug')}
        require(not any(name in text(stem) for name in COMMON_TESTS),'P13 libtest did not isolate the P14 global allocator')
        require(all(path.read_bytes() == records[0].read_bytes() for path in records), 'P14 OFF/ON/observer/storage records differ')
        require(all(path.read_bytes() == private_records[0].read_bytes() for path in private_records), 'P14 matrix bits/choice/omitted records differ')
        for storage in ('dense','compact'):
            previous.compare_counts(receipt['variants']['r1-p14-'+storage+'-baseline-observer']['bounded_activation'],
                           receipt['variants']['r1-p14-'+storage+'-candidate-observer']['bounded_activation'])
        for case in ('coaching', 'sand'):
            before, after = receipt['activation']['baseline'][case], receipt['activation']['candidate'][case]
            require((out / before['output_file']).read_bytes() == (out / after['output_file']).read_bytes(), 'P14 real search output/work differs')
            compare_counts(before['counts'], after['counts'])
        receipt.update(status='success', source_sha=SOURCE_SHA, complete_jsonl_byte_equal=True, private_jsonl_byte_equal=True,
                       actual_public_search_activated=True, R1_P13_activation_preserved=True)
    except Exception as error:
        receipt.update(status='failed', error=f'{type(error).__name__}: {error}')
        raise
    finally:
        save()
    return receipt
