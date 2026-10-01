"""Fresh bounded correctness gates before observer-OFF P1e comparisons."""
import argparse
import os
from pathlib import Path
import re
import subprocess
import tomllib

from common import (c, base_ci, binding, features, feature_args, fixed_case,
                    SOURCE_PARENT, BENCHMARK, BENCHMARK_SHA, FEATURE,
                    CONTROL_CASES, TAIL_CASE, PUBLIC_TARGET, FACTORED_TESTS, SKIP_FACTORED)
import compare_public

RESULTS = 'p1e-lazy-ko-results'
ARMS = ('original', 'off', 'on')
PUBLIC_PATH = 'engine/scenario/tests/lazy_ko_damage.rs'
MANIFEST_PATH = 'engine/scenario/Cargo.toml'
TEST_REGISTRATION = '\n[[test]]\nname = "lazy_ko_damage"\npath = "tests/lazy_ko_damage.rs"\n'

def source_root(workspace, arm):
    return workspace / ('original' if arm == 'original' else 'source')

def environment(workspace, arm):
    c.require(arm in ARMS, 'Unknown arm')
    env = base_ci.environment(workspace)
    env['CARGO_TARGET_DIR'] = str(workspace / ('target-p1e-' + arm))
    return env

def verify_original(root, bound):
    command = base_ci.command
    c.require(command(['git', 'rev-parse', 'HEAD'], root) == SOURCE_PARENT, 'Original benchmark source changed')
    c.require(command(['git', 'rev-parse', 'HEAD^'], root) == c.CANONICAL_SOURCE, 'Original source parent changed')
    # Only this test registration and the identical candidate public test may differ.
    baseline = subprocess.check_output(['git', 'show', 'HEAD:' + MANIFEST_PATH], cwd=root)
    expected = baseline + TEST_REGISTRATION.encode()
    c.require((root / MANIFEST_PATH).read_bytes() == expected, 'Original manifest differs beyond exact test registration')
    c.require(c.sha(root / PUBLIC_PATH) == bound['changed_file_sha256'][PUBLIC_PATH], 'Original public test differs')
    c.require(command(['git', 'diff', '--name-only', 'HEAD'], root).splitlines() == [MANIFEST_PATH], 'Original runtime source changed')
    c.require(command(['git', 'ls-files', '--others', '--exclude-standard'], root).splitlines() == [PUBLIC_PATH], 'Original untracked source changed')
    c.require(c.sha(root / BENCHMARK) == BENCHMARK_SHA, 'Original benchmark changed')
    return {'source_sha': SOURCE_PARENT, 'canonical_parent': c.CANONICAL_SOURCE,
            'runtime_unchanged': True, 'test_only_delta': {MANIFEST_PATH: c.sha(root / MANIFEST_PATH), PUBLIC_PATH: c.sha(root / PUBLIC_PATH)}}

def prepare_original(workspace, bound):
    root = workspace / 'original'
    c.require(base_ci.command(['git', 'status', '--porcelain'], root) == '', 'Original checkout must begin clean')
    c.require(base_ci.command(['git', 'rev-parse', 'HEAD'], root) == SOURCE_PARENT, 'Wrong original checkout')
    manifest = root / MANIFEST_PATH
    manifest.write_bytes(manifest.read_bytes() + TEST_REGISTRATION.encode())
    with (root / PUBLIC_PATH).open('xb') as stream:
        stream.write((workspace / 'source' / PUBLIC_PATH).read_bytes())
    return verify_original(root, bound)

def verify_source(root, bound):
    command = base_ci.command
    c.require(command(['git', 'rev-parse', 'HEAD'], root) == bound['source_sha'], 'Unexpected source SHA')
    c.require(command(['git', 'rev-parse', 'HEAD^'], root) == SOURCE_PARENT, 'P1e must directly inherit original benchmark source')
    c.require(command(['git', 'status', '--porcelain'], root) == '', 'Source has unreviewed changes')
    names = command(['git', 'diff', '--name-only', SOURCE_PARENT, bound['source_sha']], root).splitlines()
    c.require(set(names) == set(bound['changed_file_sha256']), 'Source delta differs from frozen allowlist')
    for name, digest in bound['changed_file_sha256'].items():
        c.require(c.sha(c.safe_file(root, name)) == digest, 'Bound candidate source changed')
    c.require(c.sha(c.safe_file(root, BENCHMARK)) == BENCHMARK_SHA, 'Inherited benchmark changed')
    manifest = tomllib.loads((root / 'engine/core/Cargo.toml').read_text())
    c.require(manifest['features'].get(FEATURE) == [] and FEATURE not in manifest['features'].get('default', []),
              'Candidate must be independent and default OFF')
    return {'source_sha': bound['source_sha'], 'parent': SOURCE_PARENT,
            'changed_file_sha256': bound['changed_file_sha256'], 'inherited_benchmark_sha256': BENCHMARK_SHA}

def fingerprints(workspace, arm, bound):
    root = workspace / ('target-p1e-' + arm) / 'release/.fingerprint'
    unit = arm != 'original' and any(arm in row['arms'] for row in bound['core_suites'])
    required = {
        'lab-engine': {'lib-lab_engine.json'} | ({'test-lib-lab_engine.json'} if unit else set()),
        'lab-scenario': {'lib-lab_scenario.json', 'bin-lab-distribution-bench.json',
                         'test-bin-lab-distribution-bench.json', 'test-integration-test-factored.json',
                         'test-integration-test-' + PUBLIC_TARGET + '.json'},
    }
    c.require(not list(root.glob('lab-search-*')), 'Unexpected search crate')
    rows = []
    for package, names in required.items():
        seen = set()
        wanted = set(features(arm == 'on')) if package == 'lab-engine' else set()
        for folder in sorted(root.glob(package + '-*')):
            for name in sorted(names):
                path = folder / name
                if not path.is_file():
                    continue
                value = c.strict_json(path.read_bytes())
                actual = value['features']
                if isinstance(actual, str):
                    actual = c.strict_json(actual)
                c.require(isinstance(actual, list) and len(actual) == len(set(actual)) and set(actual) == wanted,
                          'Compiled feature closure differs')
                c.require(value.get('rustflags') == ['-Ctarget-cpu=x86-64'], 'Compiled target flags differ')
                seen.add(name)
                rows.append({'path': str(path.relative_to(root)), 'sha256': c.sha(path),
                             'features': actual, 'content': value})
        c.require(seen == names, 'Missing actual fingerprint ' + package + ' ' + arm)
    return rows

def named_test_proof(text, tests, filtered=None):
    passed = re.findall(r'^test (\S+)(?: - should panic)? \.\.\. ok$', text, re.M)
    c.require(sorted(passed) == sorted(tests), 'Named tests missing or unexpected')
    counts = re.findall(r'^test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;', text, re.M)
    c.require(len(counts) == 1 and counts[0][:4] == (str(len(tests)), '0', '0', '0'), 'Tests incomplete')
    if filtered is not None:
        c.require(counts[0][4] == str(filtered), 'Unexpected filtered test count')
    return {'named_tests': passed, 'passed': len(passed), 'filtered': int(counts[0][4])}

def command_plan(arm, bound):
    flags = feature_args(arm == 'on')
    def test(package, target):
        return ['cargo', 'test', '--locked', '--release', '-p', package, *target, *flags]
    commands = [
        ('harness_tests', test('lab-scenario', ['--bin', 'lab-distribution-bench']) + ['--', '--test-threads=1'], list(c.TESTS), 0),
        ('public_tests', test('lab-scenario', ['--test', PUBLIC_TARGET]) + ['--', '--test-threads=1', '--show-output'], bound['public_tests'], 0),
        ('factored_tests', test('lab-scenario', ['--test', 'factored']) + ['--', '--test-threads=1',
         *[part for name in SKIP_FACTORED for part in ('--skip', name)]], list(FACTORED_TESTS), len(SKIP_FACTORED)),
    ]
    for suite in bound['core_suites']:
        if arm != 'original' and arm in suite['arms']:
            commands.append((suite['label'], test('lab-engine', ['--lib']) + [suite['filter'], '--', '--test-threads=1'], suite['tests'], None))
    commands.append(('harness_build', ['cargo', 'build', '--locked', '--release', '-p', 'lab-scenario', '--bin', 'lab-distribution-bench', *flags], None, None))
    return commands

def prepare(workspace):
    folder = workspace / RESULTS
    folder.mkdir(exist_ok=False)
    record = {'status': 'preparing', 'official_full500_metrics': None, 'full500_complete': False}
    try:
        bound = binding()
        record['source'] = verify_source(workspace / 'source', bound)
        record['original_source'] = prepare_original(workspace, bound)
        for case in (*CONTROL_CASES, TAIL_CASE):
            fixed_case(workspace, case)
        env = environment(workspace, 'off')
        rustc = base_ci.command(['rustc', '-Vv'], workspace)
        c.require(rustc.startswith('rustc 1.98.1 '), 'Frozen Rust toolchain differs')
        record.update(status='prepared', controller_sha=base_ci.command(['git', 'rev-parse', 'HEAD'], workspace / 'controller'),
                      corpus_sha256=c.CORPUS_SHA, control_cases=list(CONTROL_CASES), tail_case=TAIL_CASE,
                      features={arm: features(arm == 'on') for arm in ARMS},
                      rustflags=env['RUSTFLAGS'], benchmark_lab_environment_absent=not any(key.startswith('LAB_') for key in env),
                      public_test_only_environment='LAB_P1E_PUBLIC_RECORDS=<new per-arm artifact path>',
                      rustc=rustc, cargo=base_ci.command(['cargo', '-V'], workspace),
                      lscpu=base_ci.command(['lscpu'], workspace), available_cpus=sorted(os.sched_getaffinity(0)),
                      limits={'control_seconds': 60, 'tail_seconds': 300, 'rss_bytes': 6 * 1024**3},
                      environment={key: env[key] for key in ('RUSTFLAGS', 'CARGO_INCREMENTAL', 'CARGO_PROFILE_RELEASE_OPT_LEVEL',
                         'CARGO_PROFILE_RELEASE_DEBUG', 'CARGO_PROFILE_RELEASE_LTO', 'CARGO_PROFILE_RELEASE_CODEGEN_UNITS', 'RAYON_NUM_THREADS')})
    except Exception as error:
        record.update(status='failed', error=f'{type(error).__name__}: {error}')
        raise
    finally:
        base_ci.write(folder / 'provenance.json', record)

def build(workspace):
    folder = workspace / RESULTS
    path = folder / 'build-receipt.json'
    c.require(not path.exists(), 'Existing receipt cannot bypass fresh tests')
    record = {'status': 'building', 'cached_regression_reused': False, 'commands': [], 'arms': {}}
    try:
        bound = binding()
        verify_source(workspace / 'source', bound)
        verify_original(workspace / 'original', bound)
        c.corpus(workspace / 'controller')
        public = {}
        for arm in ARMS:
            env = environment(workspace, arm)
            target = workspace / ('target-p1e-' + arm)
            c.require(not any((target / 'release/.fingerprint').glob('lab-*')), 'Cached workspace compilation forbidden')
            public[arm] = folder / (arm + '-public-records.jsonl')
            c.require(not public[arm].exists(), 'Cached public records forbidden')
            for label, argv, tests, filtered in command_plan(arm, bound):
                log = folder / (arm + '-' + label + '.log')
                child_env = dict(env)
                if label == 'public_tests':
                    child_env['LAB_P1E_PUBLIC_RECORDS'] = str(public[arm])
                row = {'arm': arm, 'label': label, 'argv': argv, 'log': log.name,
                       'lab_environment': {k: v for k, v in child_env.items() if k.startswith('LAB_')}}
                record['commands'].append(row)
                base_ci.write(path, record)
                with log.open('xb') as stream:
                    result = subprocess.run(argv, cwd=source_root(workspace, arm) / 'engine', env=child_env,
                                            stdout=stream, stderr=subprocess.STDOUT, timeout=600)
                row.update(returncode=result.returncode, log_sha256=c.sha(log))
                c.require(result.returncode == 0, 'Fresh ' + arm + ' ' + label + ' failed')
                if tests:
                    row['test_proof'] = named_test_proof(log.read_text(), tests, filtered)
                if label == 'public_tests':
                    c.require(public[arm].is_file() and not public[arm].is_symlink(), 'Public test did not write fresh records')
                    rows, digest = compare_public.read(public[arm])
                    row.update(public_records=public[arm].name, public_records_sha256=digest, public_cases=len(rows))
            binary = target / 'release/lab-distribution-bench'
            c.require(binary.is_file() and not binary.is_symlink(), 'Missing measured binary')
            record['arms'][arm] = {'features': features(arm == 'on'), 'compiler_features': fingerprints(workspace, arm, bound),
                                   'binary': str(binary), 'binary_sha256': c.sha(binary),
                                   'public_records': public[arm].name, 'public_records_sha256': c.sha(public[arm])}
            for row in record['arms'][arm]['compiler_features']:
                original = target / 'release/.fingerprint' / row['path']
                copied = folder / 'fingerprints' / arm / row['path']
                copied.parent.mkdir(parents=True, exist_ok=True)
                with copied.open('xb') as stream:
                    stream.write(original.read_bytes())
                c.require(c.sha(copied) == row['sha256'], 'Fingerprint copy changed')
        agreement = {arm + '_vs_on': compare_public.compare_records(public[arm], public['on']) for arm in ('original', 'off')}
        c.require(all(proof['passed'] is True and proof['cases'] == 10 for proof in agreement.values()), 'Bounded public agreement failed')
        base_ci.write(folder / 'public-agreement.json', agreement)
        record.update(status='success', source_sha=bound['source_sha'], corpus_sha256=c.CORPUS_SHA,
                      public_agreement=agreement, public_agreement_sha256=c.sha(folder / 'public-agreement.json'),
                      public_comparator_sha256=bound['public_comparator_sha256'])
    except Exception as error:
        record.update(status='failed', error=f'{type(error).__name__}: {error}')
        raise
    finally:
        base_ci.write(path, record)

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('stage', choices=('prepare', 'build'))
    parser.add_argument('--workspace', required=True, type=Path)
    args = parser.parse_args()
    globals()[args.stage](args.workspace.resolve())

if __name__ == '__main__':
    main()
