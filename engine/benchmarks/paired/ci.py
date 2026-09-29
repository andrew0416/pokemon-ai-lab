"""Small CI preparation driver. No shell interpolation of dispatch inputs."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys
import tomllib


EXPERIMENT_FEATURE = 'experiment-hurt-readers'
LEAF_FEATURE = 'experiment-leaf-ending-states'
OBSERVER_FEATURE = 'experiment-leaf-ending-observer'
PREPARED_FEATURE = 'experiment-prepared-turn'
PREPARED_OBSERVER_FEATURE = 'experiment-prepared-turn-observe'
FEATURE_CHOICES = ('none', 'hurt-readers', 'leaf-ending-states', 'prepared-turn')
EXPERIMENT_FEATURES = (EXPERIMENT_FEATURE, LEAF_FEATURE, OBSERVER_FEATURE,
                       PREPARED_FEATURE, PREPARED_OBSERVER_FEATURE)
COMMAND_TIMEOUT_SECONDS = 2700
PREPARED_TESTS = (
    'matrix_reuses_real_validators_and_preserves_all_outcome_bits',
    'invalid_pair_error_order_and_deferred_support_are_identical',
    'snapshots_cannot_accept_a_stale_parent_or_ruleset',
    'normalization_suspension_mega_and_transform_match',
    'full_and_factored_paths_keep_single_turn_validation',
    'search_values_strategies_counters_budgets_and_input_restoration_match',
    'parent_error_priority_terminal_replacement_and_midturn_fallback_match',
    'one_thread_exact_deep_and_deep_nash_reuse_validation',
    'solver_full_factored_and_parallel_fallback_remains_identical',
)


def feature_args(selection, label):
    if selection not in FEATURE_CHOICES or label not in ('baseline', 'candidate'):
        raise ValueError('Invalid candidate feature or build label')
    if label == 'candidate' and selection == 'hurt-readers':
        return ['--features', 'lab-engine/' + EXPERIMENT_FEATURE]
    if selection in ('leaf-ending-states', 'prepared-turn'):
        features = 'lab-engine/' + EXPERIMENT_FEATURE
        if label == 'candidate':
            selected = LEAF_FEATURE if selection == 'leaf-ending-states' else PREPARED_FEATURE
            features += ',lab-search/' + selected
        return ['--features', features]
    return []


def read_features(manifest):
    with manifest.open('rb') as stream:
        data = tomllib.load(stream)
    features = data.get('features', {})
    if not isinstance(features, dict) or any(
            not isinstance(values, list) or any(not isinstance(value, str) for value in values)
            for values in features.values()):
        raise ValueError(f'{manifest}: invalid Cargo features table')
    return features


def reject_default_experiments(manifest, features):
    pending = list(features.get('default', []))
    visited = set()
    while pending:
        feature = pending.pop()
        if feature.rsplit('/', 1)[-1] in EXPERIMENT_FEATURES:
            raise ValueError(f'{manifest}: experiment feature must not be enabled by default')
        if feature not in visited:
            visited.add(feature)
            pending.extend(features.get(feature, []))


def verify_feature_declaration(manifest, selection):
    features = read_features(manifest)
    declared = EXPERIMENT_FEATURE in features
    if selection in ('hurt-readers', 'leaf-ending-states', 'prepared-turn') and not declared:
        raise ValueError(f'{manifest}: missing empty {EXPERIMENT_FEATURE} feature declaration')
    if declared and features[EXPERIMENT_FEATURE] != []:
        raise ValueError(f'{manifest}: {EXPERIMENT_FEATURE} must be an empty feature')
    reject_default_experiments(manifest, features)
    return {'name': EXPERIMENT_FEATURE, 'declared_empty': declared,
            'default_activation': False}


def verify_leaf_declarations(root):
    return verify_bridge_declarations(root, LEAF_FEATURE, OBSERVER_FEATURE)


def verify_prepared_declarations(root):
    return verify_bridge_declarations(root, PREPARED_FEATURE, PREPARED_OBSERVER_FEATURE)


def verify_bridge_declarations(root, feature, observer):
    declarations = {
        'core': {feature: [], observer: [feature]},
        'search': {feature: ['lab-engine/' + feature],
                   observer: [feature, 'lab-engine/' + observer]},
    }
    for package, expected in declarations.items():
        manifest = root/'engine'/package/'Cargo.toml'
        features = read_features(manifest)
        for name, values in expected.items():
            if features.get(name) != values:
                raise ValueError(f'{manifest}: unexpected {name} declaration or forwarding')
        reject_default_experiments(manifest, features)
    return declarations


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def output(argv, cwd=None):
    return subprocess.check_output(argv, cwd=cwd, text=True).strip()


def refs(workspace):
    result = workspace / 'ci-results'
    result.mkdir(exist_ok=False)
    baseline = os.environ['BASELINE_SHA']
    candidate = os.environ.get('CANDIDATE_SHA') or os.environ['GITHUB_SHA']
    for value in (baseline, candidate):
        if not re.fullmatch('[0-9a-fA-F]{40}', value):
            raise ValueError('Use a complete 40-character commit SHA from this repository.')
    suite = os.environ['SUITE']
    candidate_feature = os.environ.get('CANDIDATE_FEATURE', 'none')
    feature_args(candidate_feature, 'candidate')
    threads = int(os.environ['THREADS'])
    pairs = int(os.environ['PAIRS'])
    if suite not in ('smoke', 'narrow') or threads not in (1, 2, 4):
        raise ValueError('Invalid suite or thread count')
    if pairs < 2 or pairs > 20 or pairs % 2:
        raise ValueError('pairs must be even, from 2 through 20')
    cpus = len(os.sched_getaffinity(0)) if hasattr(os, 'sched_getaffinity') else os.cpu_count()
    if threads > cpus:
        raise ValueError('Requested threads exceed CPUs available to this runner')
    metadata = {'baseline_sha': baseline.lower(), 'candidate_sha': candidate.lower(),
                'workflow_sha': os.environ['GITHUB_SHA'], 'suite': suite, 'threads': threads,
                'pairs': pairs, 'run_id': os.environ['GITHUB_RUN_ID'],
                'run_attempt': os.environ['GITHUB_RUN_ATTEMPT'], 'available_cpus': cpus,
                'candidate_feature': candidate_feature,
                'feature_args': {label: feature_args(candidate_feature, label)
                                 for label in ('baseline', 'candidate')}}
    (result/'request.json').write_text(json.dumps(metadata, indent=2)+'\n', encoding='utf-8')
    with open(os.environ['GITHUB_OUTPUT'], 'a', encoding='utf-8') as stream:
        for key in ('baseline_sha', 'candidate_sha'):
            stream.write(f'{key}={metadata[key]}\n')


def prepare(workspace):
    result = workspace/'ci-results'
    metadata = json.loads((result/'request.json').read_text(encoding='utf-8'))
    selection = metadata['candidate_feature']
    if metadata['feature_args'] != {label: feature_args(selection, label)
                                   for label in ('baseline', 'candidate')}:
        raise ValueError('Requested feature arguments do not match the explicit selection')
    harness = Path(__file__).with_name('harness.rs')
    metadata['harness_sha256'] = sha(harness)
    metadata['rustc'] = output(['rustc', '-Vv'])
    metadata['cargo'] = output(['cargo', '-V'])
    metadata['build_environment'] = {key: value for key, value in sorted(os.environ.items())
                                     if key.startswith('CARGO_PROFILE_RELEASE_') or key in
                                     ('RUSTFLAGS', 'CARGO_INCREMENTAL', 'CARGO_BUILD_JOBS')}
    metadata['sources'] = {}
    for label in ('baseline', 'candidate'):
        root = workspace/label
        actual = output(['git', 'rev-parse', 'HEAD'], root)
        if actual != metadata[f'{label}_sha']:
            raise ValueError(f'{label} checkout does not match requested SHA')
        if output(['git', 'status', '--porcelain'], root):
            raise ValueError(f'{label} checkout is not clean before harness injection')
        destination = root/'engine/search/examples/ci_bench.rs'
        if destination.exists():
            raise ValueError('Reserved example ci_bench.rs already exists in source revision')
        metadata['sources'][label] = {
            'commit': actual, 'lock_sha256': sha(root/'engine/Cargo.lock'),
            'experiment_feature': verify_feature_declaration(root/'engine/core/Cargo.toml', selection),
            'workspace_manifest_sha256': sha(root/'engine/Cargo.toml'),
            'search_manifest_sha256': sha(root/'engine/search/Cargo.toml'),
            'package_manifests': {name: sha(root/'engine'/name/'Cargo.toml')
                                  for name in ('core', 'scenario', 'py')},
            'cargo_configuration': {name: sha(root/name) for name in (
                '.cargo/config', '.cargo/config.toml', 'rust-toolchain', 'rust-toolchain.toml',
                'engine/.cargo/config', 'engine/.cargo/config.toml',
                'engine/rust-toolchain', 'engine/rust-toolchain.toml') if (root/name).is_file()}}
        if selection == 'leaf-ending-states':
            metadata['sources'][label]['leaf_declarations'] = verify_leaf_declarations(root)
        elif selection == 'prepared-turn':
            metadata['sources'][label]['prepared_declarations'] = verify_prepared_declarations(root)
    # A dependency/profile change needs a separately designed experiment.
    for key in ('lock_sha256', 'workspace_manifest_sha256', 'search_manifest_sha256',
                'package_manifests', 'cargo_configuration'):
        if metadata['sources']['baseline'][key] != metadata['sources']['candidate'][key]:
            raise ValueError(f'Baseline/candidate differ in {key}; strict source-only benchmark refused')
    for label in ('baseline', 'candidate'):
        destination = workspace/label/'engine/search/examples/ci_bench.rs'
        destination.parent.mkdir(exist_ok=True)
        shutil.copyfile(harness, destination)
    (result/'provenance.json').write_text(json.dumps(metadata, indent=2)+'\n', encoding='utf-8')


def build_commands(suite, selection, label):
    if suite not in ('smoke', 'narrow'):
        raise ValueError('Invalid benchmark suite')
    tests = [['cargo', 'test', '--locked', '--release', '-p', 'lab-engine', '-p', 'lab-scenario', '-p', 'lab-search']]
    if suite == 'smoke':
        # Infrastructure checks need the harness and its fixture, not every oracle binary.
        # Actual candidate comparisons (narrow) retain the full regression gate above.
        tests = [
            ['cargo', 'test', '--locked', '--release', '-p', 'lab-engine', '-p', 'lab-search', '--lib'],
            ['cargo', 'test', '--locked', '--release', '-p', 'lab-scenario', '--test', 'abilities_slow_start_truant'],
        ]
    commands = tests + [
        ['cargo', 'build', '--locked', '--release', '-p', 'lab-search', '--example', 'ci_bench'],
    ]
    return [argv + feature_args(selection, label) for argv in commands]


def fingerprint_expectations(selection, label):
    feature_args(selection, label)
    hurt_active = selection in ('leaf-ending-states', 'prepared-turn') or (
        label == 'candidate' and selection == 'hurt-readers')
    leaf_active = selection == 'leaf-ending-states' and label == 'candidate'
    prepared_active = selection == 'prepared-turn' and label == 'candidate'
    bridge_expected = {LEAF_FEATURE: leaf_active, OBSERVER_FEATURE: False,
                       PREPARED_FEATURE: prepared_active, PREPARED_OBSERVER_FEATURE: False}
    packages = {'lab-engine': ('lib-lab_engine.json', 'test-lib-lab_engine.json')}
    expected = {'lab-engine': {EXPERIMENT_FEATURE: hurt_active, **bridge_expected}}
    if selection in ('leaf-ending-states', 'prepared-turn'):
        packages['lab-search'] = ('lib-lab_search.json', 'test-lib-lab_search.json', 'example-ci_bench.json')
        expected['lab-search'] = dict(bridge_expected)
    return packages, expected, hurt_active


def preserve_fingerprints(workspace, label, selection):
    packages, expected, hurt_active = fingerprint_expectations(selection, label)
    return preserve_expected_fingerprints(workspace, label, packages, expected, hurt_active)


def preserve_expected_fingerprints(workspace, label, packages, expected, hurt_active):
    target = workspace/('target-' + label)
    result = workspace/'ci-results'
    evidence = {'expected_active': hurt_active, 'expected_by_package': expected,
                'feature': EXPERIMENT_FEATURE, 'fingerprints': []}
    fingerprint_root = target/'release/.fingerprint'
    for package, names in packages.items():
        for directory in sorted(fingerprint_root.glob(package + '-*')):
            for name in names:
                source = directory/name
                if not source.is_file():
                    continue
                destination = result/'fingerprints'/label/directory.name/name
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(source, destination)
                data = json.loads(source.read_text(encoding='utf-8'))
                features = data.get('features')
                if isinstance(features, str):
                    features = json.loads(features)
                if not isinstance(features, list) or any(not isinstance(value, str) for value in features):
                    raise ValueError(f'{source}: Cargo fingerprint has no valid feature list')
                evidence['fingerprints'].append({
                    'package': package, 'kind': name, 'features': features, 'sha256': sha(source),
                    'target_path': source.relative_to(target).as_posix(),
                    'artifact_path': destination.relative_to(result).as_posix(),
                })
    # Write evidence before asserting, so a failed activation check remains inspectable.
    (result/f'{label}-features.json').write_text(json.dumps(evidence, indent=2)+'\n', encoding='utf-8')
    for package, names in packages.items():
        kinds = {item['kind'] for item in evidence['fingerprints'] if item['package'] == package}
        if kinds != set(names):
            raise ValueError(f'{label}: missing compiled {package} fingerprints: {set(names) - kinds}')
    for item in evidence['fingerprints']:
        for feature, active in expected[item['package']].items():
            if (feature in item['features']) != active:
                raise ValueError(f'{label}: actual compiled feature activation differs from request: '
                                 f'{item["target_path"]}: {item["features"]}')
    return evidence


def prepared_validation_command():
    return ['cargo', 'test', '--locked', '--release', '-p', 'lab-search',
            '--test', 'prepared_turn', '--features',
            'lab-engine/' + EXPERIMENT_FEATURE + ',lab-search/' + PREPARED_OBSERVER_FEATURE]


def validate_prepared_turn(workspace):
    """Run observer-dependent differential tests outside both timing targets."""
    result = workspace/'ci-results'
    label = 'prepared-validation'
    target = workspace/('target-' + label)
    if target.exists():
        raise ValueError('Prepared validation target directory must be new')
    command = prepared_validation_command()
    log_path = result/(label + '.log')
    receipt_path = result/(label + '.json')
    receipt = {'status': 'running', 'command': command, 'source': 'candidate',
               'target_directory': str(target), 'log': log_path.name,
               'per_command_timeout_seconds': COMMAND_TIMEOUT_SECONDS,
               'expected_tests': list(PREPARED_TESTS)}

    def save():
        receipt_path.write_text(json.dumps(receipt, indent=2)+'\n', encoding='utf-8')

    save()
    try:
        env = os.environ.copy()
        env['CARGO_TARGET_DIR'] = str(target)
        with log_path.open('w', encoding='utf-8') as log:
            log.write('COMMAND '+json.dumps(command)+'\n')
            log.flush()
            print(f'{label}: {" ".join(command)}', flush=True)
            proc = subprocess.Popen(command, cwd=workspace/'candidate/engine', env=env,
                                    stdout=log, stderr=subprocess.STDOUT,
                                    start_new_session=(os.name == 'posix'))
            try:
                proc.wait(timeout=COMMAND_TIMEOUT_SECONDS)
            except subprocess.TimeoutExpired:
                if os.name == 'posix':
                    os.killpg(proc.pid, signal.SIGKILL)
                else:
                    proc.kill()
                proc.wait()
                raise RuntimeError('Prepared validation timed out; process group terminated')
            receipt['returncode'] = proc.returncode
        if proc.returncode:
            raise RuntimeError(f'Prepared validation failed ({proc.returncode}); see artifact log')
        log_text = log_path.read_text(encoding='utf-8')
        passed = re.findall(r'^test (\S+) \.\.\. ok$', log_text, re.MULTILINE)
        receipt['passed_tests'] = passed
        if sorted(passed) != sorted(PREPARED_TESTS) or not re.search(
                r'^test result: ok\. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;',
                log_text, re.MULTILINE):
            raise ValueError('Prepared validation must execute all nine named tests, with none skipped')
        bridge = {LEAF_FEATURE: False, OBSERVER_FEATURE: False,
                  PREPARED_FEATURE: True, PREPARED_OBSERVER_FEATURE: True}
        receipt['compiler_feature_evidence'] = preserve_expected_fingerprints(
            workspace, label,
            {'lab-engine': ('lib-lab_engine.json',),
             'lab-search': ('lib-lab_search.json', 'test-integration-test-prepared_turn.json')},
            {'lab-engine': {EXPERIMENT_FEATURE: True, **bridge}, 'lab-search': dict(bridge)}, True)
        receipt['status'] = 'ok'
        save()
        return receipt
    except Exception as error:
        receipt.update(status='failed', error=f'{type(error).__name__}: {error}')
        save()
        raise


def build(workspace):
    # Finish ALL tests/builds and verify actual compiler features before timing.
    import build_cache

    result = workspace/'ci-results'
    request = json.loads((result/'request.json').read_text(encoding='utf-8'))
    selection = request['candidate_feature']
    commands_by_version = {label: build_commands(request['suite'], selection, label)
                           for label in ('baseline', 'candidate')}
    expected_args = {label: feature_args(selection, label) for label in commands_by_version}
    if request['feature_args'] != expected_args:
        raise ValueError('Requested feature arguments do not match the explicit selection')
    plan = {'suite': request['suite'], 'candidate_feature': selection,
            'feature_args': expected_args, 'commands_by_version': commands_by_version,
            'per_command_timeout_seconds': COMMAND_TIMEOUT_SECONDS}
    if selection == 'prepared-turn':
        plan['prepared_validation_command'] = prepared_validation_command()
    (result/'test-plan.json').write_text(json.dumps(plan, indent=2)+'\n', encoding='utf-8')
    provenance = json.loads((result/'provenance.json').read_text(encoding='utf-8'))
    provenance['build_plan'] = plan
    provenance['compiler_feature_evidence'] = {}
    (result/'provenance.json').write_text(json.dumps(provenance, indent=2)+'\n', encoding='utf-8')
    for label in ('baseline', 'candidate'):
        env = os.environ.copy()
        target = workspace/('target-'+label)
        if target.exists():
            raise ValueError(f'{label}: target directory must be new to establish fresh build evidence')
        env['CARGO_TARGET_DIR'] = str(target)
        reused = build_cache.restore(workspace, label)
        receipt = {'schema_version': 1, 'status': 'running', 'label': label,
                   'suite': request['suite'], 'selection': selection, 'reused': reused,
                   'commands': [], 'log': label + '-build.log',
                   'feature_evidence': label + '-features.json'}
        receipt_path = result/(label + '-build-receipt.json')
        try:
            if reused:
                receipt['reused_from_run'] = json.loads(
                    (result/('cache-' + label + '.json')).read_text(encoding='utf-8'))['reused_from_run']
                print(f'{label}: reused verified executable; regressions were run in '
                      f'{receipt["reused_from_run"]["run_id"]}', flush=True)
            else:
                with (result/f'{label}-build.log').open('w', encoding='utf-8') as log:
                    for argv in commands_by_version[label]:
                        print(f'{label}: {" ".join(argv)}', flush=True)
                        log.write('COMMAND '+json.dumps(argv)+'\n')
                        log.flush()
                        proc = subprocess.Popen(argv, cwd=workspace/label/'engine', env=env,
                                                stdout=log, stderr=subprocess.STDOUT,
                                                start_new_session=(os.name == 'posix'))
                        try:
                            proc.wait(timeout=COMMAND_TIMEOUT_SECONDS)
                        except subprocess.TimeoutExpired:
                            if os.name == 'posix':
                                os.killpg(proc.pid, signal.SIGKILL)
                            else:
                                proc.kill()
                            proc.wait()
                            raise RuntimeError(f'{label} build/test timed out; process group terminated')
                        receipt['commands'].append({'argv': argv, 'returncode': proc.returncode})
                        if proc.returncode:
                            log.flush()
                            print((result/f'{label}-build.log').read_text(encoding='utf-8')[-12000:])
                            raise RuntimeError(f'{label} build/test failed ({proc.returncode}); see artifact log')
            provenance['compiler_feature_evidence'][label] = preserve_fingerprints(workspace, label, selection)
            receipt['status'] = 'success'
            receipt_path.write_text(json.dumps(receipt, indent=2)+'\n', encoding='utf-8')
        except Exception as error:
            receipt.update(status='failed', error=f'{type(error).__name__}: {error}')
            receipt_path.write_text(json.dumps(receipt, indent=2)+'\n', encoding='utf-8')
            raise
        if not reused:
            build_cache.seal(workspace, label, provenance['compiler_feature_evidence'][label])
        provenance.setdefault('build_cache', {})[label] = json.loads(
            (result/('cache-' + label + '.json')).read_text(encoding='utf-8'))
        (result/'provenance.json').write_text(json.dumps(provenance, indent=2)+'\n', encoding='utf-8')
    if selection == 'prepared-turn':
        provenance['prepared_validation'] = validate_prepared_turn(workspace)
        (result/'provenance.json').write_text(json.dumps(provenance, indent=2)+'\n', encoding='utf-8')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('stage', choices=('refs', 'prepare', 'build'))
    parser.add_argument('--workspace', type=Path, required=True)
    args = parser.parse_args()
    try:
        globals()[args.stage](args.workspace.resolve())
    except Exception as error:
        result = args.workspace/'ci-results'
        result.mkdir(exist_ok=True)
        (result/(args.stage+'-error.txt')).write_text(f'{type(error).__name__}: {error}\n', encoding='utf-8')
        raise


if __name__ == '__main__':
    main()
