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
COMPACT_FEATURE = 'experiment-compact-volatiles'
COMPACT_PROBE_PATH = 'engine/scenario/examples/ci_compact_probe.rs'
FEATURE_CHOICES = ('none', 'hurt-readers', 'leaf-ending-states', 'prepared-turn', 'compact-volatiles',
                   'all-optimizations', 'replay-action-keys', 'slot-diff', 'stats-off-cost', 'p8def-combined', 'p8d-vs-p8def', 'borrowed-child-keys', 'matrix-pass-through')
EXPERIMENT_FEATURES = (EXPERIMENT_FEATURE, LEAF_FEATURE, OBSERVER_FEATURE,
                       PREPARED_FEATURE, PREPARED_OBSERVER_FEATURE, COMPACT_FEATURE)
NEW_MODES = ('replay-action-keys', 'slot-diff', 'stats-off-cost')
P8DEF_COMBINED = 'p8def-combined'
P8D_VS_P8DEF = 'p8d-vs-p8def'
COMBINED_NEW_MODES = (P8DEF_COMBINED, P8D_VS_P8DEF)
P8DEF_MODES = (*NEW_MODES, *COMBINED_NEW_MODES)
BORROWED_MODE = 'borrowed-child-keys'
BORROWED_FEATURE = 'experiment-borrowed-child-keys'
BORROWED_OBSERVER = BORROWED_FEATURE + '-observer'
MATRIX_MODE = 'matrix-pass-through'
MATRIX_FEATURE = 'experiment-matrix-pass-through'
MATRIX_OBSERVER = MATRIX_FEATURE + '-observer'
STRICT_MODES = (*P8DEF_MODES, BORROWED_MODE, MATRIX_MODE)
NEW_FEATURES = {mode: 'experiment-' + mode for mode in NEW_MODES}
NEW_OBSERVERS = {mode: feature + '-observer' for mode, feature in NEW_FEATURES.items()}
ALL_EXPERIMENT_FEATURES = (*EXPERIMENT_FEATURES, *NEW_FEATURES.values(), *NEW_OBSERVERS.values(),
                           BORROWED_FEATURE, BORROWED_OBSERVER, MATRIX_FEATURE, MATRIX_OBSERVER)
PACKAGE_BASE_FEATURES = {'lab-engine': set(), 'lab-scenario': set(),
                         'lab-search': {'cli', 'default', 'lab-scenario', 'scenario', 'serde_json'}}
# Filled from the implemented engine's exact Rust test registration before dispatch.
# Empty contracts fail closed; they can never count as a passed activation gate.
NEW_OBSERVER_TESTS = {mode: () for mode in NEW_MODES}
NEW_OBSERVER_TESTS['replay-action-keys'] = (
    'ordinary_keys_activate_without_changing_stage_work_or_exact_results',
    'resumed_hits_and_midturn_switches_keep_the_uncached_path',
    'bounded_full_and_factored_paths_preserve_bits_and_activation_scope',
    'in_stage_errors_and_successes_preserve_original_inputs',
    'sampling_remains_seed_identical_and_never_reuses',
)
NEW_OBSERVER_TESTS['slot-diff'] = (
    'scalar_diffs_preserve_full_states_and_incremental_hash_in_singles_and_doubles',
    'shortcut_avoids_real_switch_and_compact_clone_allocations',
    'every_other_slot_field_retains_the_exact_baseline_fallback',
    'empty_slots_unchanged_slots_and_inactive_payload_keep_existing_boundary',
    'dynamax_keeps_the_existing_unsupported_reconstruction_boundary',
    'real_turns_match_baseline_full_state_order_probability_and_suspension',
)
NEW_OBSERVER_TESTS['stats-off-cost'] = (
    'turn::stats_off_cost::tests::env_presence_snapshot_and_diagnostics_are_process_isolated',
)
NEW_OBSERVER_TARGETS = {
    'replay-action-keys': {'package': 'lab-scenario', 'kind': 'test', 'target': 'replay_action_keys'},
    'slot-diff': {'package': 'lab-scenario', 'kind': 'test', 'target': 'p8e_slot_diff',
                  'additional_package': 'lab-engine', 'test_counts': (5, 1)},
    'stats-off-cost': {'package': 'lab-engine', 'kind': 'lib'},
}
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


def new_runtime_modes(selection, label):
    if selection not in P8DEF_MODES or label not in ('baseline', 'candidate'):
        raise ValueError('Invalid P8d/e/f selection or build label')
    if label == 'baseline':
        return ('replay-action-keys',) if selection == P8D_VS_P8DEF else ()
    return NEW_MODES if selection in COMBINED_NEW_MODES else (selection,)


def feature_args(selection, label):
    if selection not in FEATURE_CHOICES or label not in ('baseline', 'candidate'):
        raise ValueError('Invalid candidate feature or build label')
    if selection == MATRIX_MODE:
        features = feature_args(P8D_VS_P8DEF, 'baseline')[1]
        if label == 'candidate':
            features += ',lab-search/' + MATRIX_FEATURE
        return ['--features', features]
    if selection == BORROWED_MODE:
        features = feature_args(P8D_VS_P8DEF, 'baseline')[1]
        if label == 'candidate':
            features += ',lab-search/' + BORROWED_FEATURE
        return ['--features', features]
    if selection in P8DEF_MODES:
        features = feature_args('all-optimizations', 'candidate')[1]
        features += ''.join(',lab-engine/' + NEW_FEATURES[mode]
                            for mode in new_runtime_modes(selection, label))
        return ['--features', features]
    if selection == 'all-optimizations':
        if label == 'baseline':
            return []
        return ['--features', ','.join(('lab-engine/' + EXPERIMENT_FEATURE,
                                       'lab-search/' + PREPARED_FEATURE,
                                       'lab-search/' + LEAF_FEATURE,
                                       'lab-engine/' + COMPACT_FEATURE))]
    if label == 'candidate' and selection == 'hurt-readers':
        return ['--features', 'lab-engine/' + EXPERIMENT_FEATURE]
    if selection == 'compact-volatiles':
        features = 'lab-engine/' + EXPERIMENT_FEATURE
        if label == 'candidate':
            features += ',lab-engine/' + COMPACT_FEATURE
        return ['--features', features]
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
        if feature.rsplit('/', 1)[-1] in ALL_EXPERIMENT_FEATURES:
            raise ValueError(f'{manifest}: experiment feature must not be enabled by default')
        if feature not in visited:
            visited.add(feature)
            pending.extend(features.get(feature, []))


def verify_feature_declaration(manifest, selection):
    features = read_features(manifest)
    declared = EXPERIMENT_FEATURE in features
    if selection in FEATURE_CHOICES[1:] and not declared:
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


def verify_compact_declarations(root):
    core_manifest = root/'engine/core/Cargo.toml'
    verify_feature_declaration(core_manifest, 'compact-volatiles')
    core = read_features(core_manifest)
    if core.get(COMPACT_FEATURE) != []:
        raise ValueError(f'{core_manifest}: {COMPACT_FEATURE} must be declared empty')
    reject_default_experiments(core_manifest, core)
    search_manifest = root/'engine/search/Cargo.toml'
    search = read_features(search_manifest)
    if COMPACT_FEATURE in search or any(
            value.rsplit('/', 1)[-1] == COMPACT_FEATURE for values in search.values() for value in values):
        raise ValueError(f'{search_manifest}: compact mode must not declare a search feature or forwarding')
    reject_default_experiments(search_manifest, search)
    return {'core': {COMPACT_FEATURE: []}, 'search_forwarding': False,
            'default_activation': False}


def verify_new_declarations(root, selection):
    if selection not in NEW_MODES:
        raise ValueError('Invalid independent candidate mode')
    core_path, search_path = root/'engine/core/Cargo.toml', root/'engine/search/Cargo.toml'
    core, search = read_features(core_path), read_features(search_path)
    runtime, observer = NEW_FEATURES[selection], NEW_OBSERVERS[selection]
    if core.get(runtime) != [] or core.get(observer) != [runtime]:
        raise ValueError(f'{core_path}: missing or invalid {runtime}/{observer} declarations')
    for mode in NEW_MODES:
        flag, diagnostic = NEW_FEATURES[mode], NEW_OBSERVERS[mode]
        if flag in core or diagnostic in core:
            if core.get(flag) != [] or core.get(diagnostic) != [flag]:
                raise ValueError(f'{core_path}: unexpected independent feature declaration')
    forbidden = set(NEW_FEATURES.values()) | set(NEW_OBSERVERS.values())
    if forbidden.intersection(search) or any(
            value.rsplit('/', 1)[-1] in forbidden for values in search.values() for value in values):
        raise ValueError(f'{search_path}: independent candidates must remain core-only')
    for manifest, features in ((core_path, core), (search_path, search)):
        reject_default_experiments(manifest, features)
    target = NEW_OBSERVER_TARGETS[selection]
    if target['package'] == 'lab-scenario':
        scenario_path = root/'engine/scenario/Cargo.toml'
        scenario = read_features(scenario_path)
        if scenario.get(observer) != ['lab-engine/' + observer]:
            raise ValueError(f'{scenario_path}: observer must forward exactly the core observer')
        reject_default_experiments(scenario_path, scenario)
        with scenario_path.open('rb') as stream:
            manifest_data = tomllib.load(stream)
        targets = [row for row in manifest_data.get('test', []) if row.get('name') == target['target']]
        if (len(targets) != 1 or targets[0].get('path') != 'tests/' + target['target'] + '.rs'
                or targets[0].get('required-features') != [observer]):
            raise ValueError(f'{scenario_path}: exact observer integration target is not registered')
    return {'core': {runtime: [], observer: [runtime]}, 'search_forwarding': False,
            'default_activation': False}


def injected_sources(selection):
    """The only controller files allowed to be added to each prepared checkout."""
    if selection not in FEATURE_CHOICES:
        raise ValueError('Invalid candidate feature')
    files = {'engine/search/examples/ci_bench.rs': Path(__file__).with_name('harness.rs')}
    if selection in ('compact-volatiles', 'all-optimizations', *STRICT_MODES):
        files[COMPACT_PROBE_PATH] = Path(__file__).with_name('compact_probe.rs')
    if selection == BORROWED_MODE:
        files['engine/search/examples/ci_borrowed_child_keys_observer.rs'] = Path(__file__).with_name('borrowed_child_keys_probe.rs')
    if selection == MATRIX_MODE:
        files['engine/search/examples/ci_matrix_pass_through_observer.rs'] = Path(__file__).with_name('matrix_pass_through_probe.rs')
    return files


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
    if candidate_feature in ('compact-volatiles', 'all-optimizations', *STRICT_MODES) and suite != 'narrow':
        raise ValueError(f'{candidate_feature} requires the full narrow regression suite')
    if candidate_feature in ('all-optimizations', *STRICT_MODES) and baseline.lower() != candidate.lower():
        raise ValueError(f'{candidate_feature} requires the same source SHA with features off/on')
    if candidate_feature == BORROWED_MODE:
        from borrowed_child_keys import SOURCE_SHA
        if baseline.lower() != SOURCE_SHA or threads != 1 or pairs != 10:
            raise ValueError('P13 requires frozen source, one thread and ten pairs')
    if candidate_feature == MATRIX_MODE:
        from matrix_pass_through import SOURCE_SHA
        if baseline.lower() != SOURCE_SHA or threads != 1 or pairs != 10:
            raise ValueError('P14 requires frozen source, one thread and ten pairs')
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
    if selection in ('all-optimizations', *STRICT_MODES) and metadata['baseline_sha'] != metadata['candidate_sha']:
        raise ValueError(f'{selection} requires the same source SHA with features off/on')
    if metadata['feature_args'] != {label: feature_args(selection, label)
                                   for label in ('baseline', 'candidate')}:
        raise ValueError('Requested feature arguments do not match the explicit selection')
    harness = Path(__file__).with_name('harness.rs')
    injections = injected_sources(selection)
    metadata['harness_sha256'] = sha(harness)
    metadata['injected_source_sha256'] = {name: sha(source) for name, source in injections.items()}
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
        for name in injections:
            destination = root/name
            if destination.exists() or destination.is_symlink():
                raise ValueError(f'Reserved injected example already exists in source revision: {name}')
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
        if selection in ('leaf-ending-states', 'all-optimizations', *STRICT_MODES):
            metadata['sources'][label]['leaf_declarations'] = verify_leaf_declarations(root)
        if selection in ('prepared-turn', 'all-optimizations', *STRICT_MODES):
            metadata['sources'][label]['prepared_declarations'] = verify_prepared_declarations(root)
        if selection in ('compact-volatiles', 'all-optimizations', *STRICT_MODES):
            metadata['sources'][label]['compact_declarations'] = verify_compact_declarations(root)
        if selection in NEW_MODES:
            metadata['sources'][label]['independent_candidate_declarations'] = verify_new_declarations(root, selection)
        elif selection in COMBINED_NEW_MODES:
            metadata['sources'][label]['combined_candidate_declarations'] = {
                mode: verify_new_declarations(root, mode) for mode in NEW_MODES}
        elif selection == MATRIX_MODE:
            import matrix_pass_through
            metadata['sources'][label]['matrix_pass_through_declarations'] = matrix_pass_through.verify_declarations(root)
            metadata['sources'][label]['replay_declarations'] = verify_new_declarations(root, 'replay-action-keys')
        elif selection == BORROWED_MODE:
            import borrowed_child_keys
            metadata['sources'][label]['borrowed_child_keys_declarations'] = borrowed_child_keys.verify_declarations(root)
            metadata['sources'][label]['replay_declarations'] = verify_new_declarations(root, 'replay-action-keys')
    # A dependency/profile change needs a separately designed experiment.
    for key in ('lock_sha256', 'workspace_manifest_sha256', 'search_manifest_sha256',
                'package_manifests', 'cargo_configuration'):
        if metadata['sources']['baseline'][key] != metadata['sources']['candidate'][key]:
            raise ValueError(f'Baseline/candidate differ in {key}; strict source-only benchmark refused')
    for label in ('baseline', 'candidate'):
        for name, source in injections.items():
            destination = workspace/label/name
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, destination)
    (result/'provenance.json').write_text(json.dumps(metadata, indent=2)+'\n', encoding='utf-8')


def build_commands(suite, selection, label):
    if suite not in ('smoke', 'narrow'):
        raise ValueError('Invalid benchmark suite')
    if selection in ('compact-volatiles', 'all-optimizations', *STRICT_MODES) and suite != 'narrow':
        raise ValueError(f'{selection} requires the full narrow regression suite')
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
    return [argv + ['--timings'] + feature_args(selection, label) +
            (['--', '--test-threads=1'] if selection in ('compact-volatiles', 'all-optimizations', *STRICT_MODES) and argv[1] == 'test' else [])
            for argv in commands]


def verify_prepared(workspace):
    """Detect source/lock mutation by cache actions after harness injection."""
    result = workspace/'ci-results'
    request = json.loads((result/'request.json').read_text(encoding='utf-8'))
    provenance = json.loads((result/'provenance.json').read_text(encoding='utf-8'))
    harness_hash = sha(Path(__file__).with_name('harness.rs'))
    selection = request.get('candidate_feature', 'none')
    injections = injected_sources(selection)
    receipt = {'schema_version': 1, 'status': 'running', 'sources': {}}
    receipt_path = result/'prepared-source-verification.json'
    try:
        if selection in STRICT_MODES or provenance.get('candidate_feature') in STRICT_MODES:
            declared = {label: feature_args(selection, label) for label in ('baseline', 'candidate')}
            if (request.get('candidate_feature') != provenance.get('candidate_feature')
                    or request.get('feature_args') != declared or provenance.get('feature_args') != declared
                    or request['baseline_sha'] != request['candidate_sha']):
                raise ValueError('Independent candidate request changed after preparation')
        if harness_hash != provenance['harness_sha256']:
            raise ValueError('Controller harness changed after prepare')
        injection_hashes = {name: sha(source) for name, source in injections.items()}
        if (selection in ('compact-volatiles', 'all-optimizations', *STRICT_MODES) or 'injected_source_sha256' in provenance) and (
                injection_hashes != provenance.get('injected_source_sha256')):
            raise ValueError('Controller injected source changed after prepare')
        for label in ('baseline', 'candidate'):
            root = workspace/label
            expected = provenance['sources'][label]
            actual = output(['git', 'rev-parse', 'HEAD'], root)
            if actual != expected['commit'] or actual != request[label + '_sha']:
                raise ValueError(f'{label}: HEAD changed after prepare')
            if output(['git', 'status', '--porcelain', '--untracked-files=no'], root):
                raise ValueError(f'{label}: tracked source changed after prepare')
            untracked = output(['git', 'ls-files', '--others', '--exclude-standard'], root).splitlines()
            if sorted(untracked) != sorted(injections):
                raise ValueError(f'{label}: unexpected untracked source after prepare')
            observed = {
                'lock_sha256': sha(root/'engine/Cargo.lock'),
                'workspace_manifest_sha256': sha(root/'engine/Cargo.toml'),
                'search_manifest_sha256': sha(root/'engine/search/Cargo.toml'),
                'package_manifests': {name: sha(root/'engine'/name/'Cargo.toml')
                                      for name in ('core', 'scenario', 'py')},
                'cargo_configuration': {name: sha(root/name) for name in (
                    '.cargo/config', '.cargo/config.toml', 'rust-toolchain', 'rust-toolchain.toml',
                    'engine/.cargo/config', 'engine/.cargo/config.toml',
                    'engine/rust-toolchain', 'engine/rust-toolchain.toml') if (root/name).is_file()}}
            for key, value in observed.items():
                if value != expected[key]:
                    raise ValueError(f'{label}: {key} changed after prepare')
            for name, expected_hash in injection_hashes.items():
                injected = root/name
                if injected.is_symlink() or sha(injected) != expected_hash:
                    raise ValueError(f'{label}: injected source changed after prepare: {name}')
            receipt['sources'][label] = {'commit': actual, **observed,
                                         'harness_sha256': harness_hash,
                                         'injected_source_sha256': injection_hashes}
        receipt['status'] = 'success'
    except Exception as error:
        receipt.update(status='failed', error=f'{type(error).__name__}: {error}')
        raise
    finally:
        receipt_path.write_text(json.dumps(receipt, indent=2)+'\n', encoding='utf-8')
    return receipt


def timing_snapshot(target):
    folder = target/'cargo-timings'
    if folder.is_symlink() or not folder.is_dir():
        return {}
    return {path.name: sha(path) for path in folder.glob('*.html')
            if path.is_file() and not path.is_symlink()}


def preserve_build_timings(workspace, label, before):
    """Keep fresh Cargo compilation reports on both successful and failed builds."""
    target = workspace/('target-' + label)
    current = timing_snapshot(target)
    files = []
    for name, digest in sorted(current.items()):
        if before.get(name) == digest:
            continue
        destination = workspace/'ci-results/build-timings'/label/name
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(target/'cargo-timings'/name, destination)
        files.append({'path': destination.relative_to(workspace/'ci-results').as_posix(),
                      'sha256': digest})
    return files


def fingerprint_expectations(selection, label):
    feature_args(selection, label)
    if selection in STRICT_MODES:
        core = {name: False for name in ALL_EXPERIMENT_FEATURES}
        core.update({EXPERIMENT_FEATURE: True, PREPARED_FEATURE: True,
                     LEAF_FEATURE: True, COMPACT_FEATURE: True})
        modes = ('replay-action-keys',) if selection in (BORROWED_MODE, MATRIX_MODE) else new_runtime_modes(selection, label)
        for mode in modes:
            core[NEW_FEATURES[mode]] = True
        if selection == BORROWED_MODE:
            core[BORROWED_FEATURE] = label == 'candidate'
        search = {name: False for name in ALL_EXPERIMENT_FEATURES}
        search.update({PREPARED_FEATURE: True, LEAF_FEATURE: True})
        if selection == BORROWED_MODE:
            search[BORROWED_FEATURE] = label == 'candidate'
        if selection == MATRIX_MODE:
            search[MATRIX_FEATURE] = label == 'candidate'
        return ({'lab-engine': ('lib-lab_engine.json', 'test-lib-lab_engine.json'),
                 'lab-search': ('lib-lab_search.json', 'test-lib-lab_search.json', 'example-ci_bench.json')},
                {'lab-engine': core, 'lab-search': search}, True)
    if selection == 'all-optimizations':
        active = label == 'candidate'
        core = {name: False for name in EXPERIMENT_FEATURES}
        core.update({EXPERIMENT_FEATURE: active, PREPARED_FEATURE: active,
                     LEAF_FEATURE: active, COMPACT_FEATURE: active})
        search = {name: False for name in EXPERIMENT_FEATURES}
        search.update({PREPARED_FEATURE: active, LEAF_FEATURE: active})
        return ({'lab-engine': ('lib-lab_engine.json', 'test-lib-lab_engine.json'),
                 'lab-search': ('lib-lab_search.json', 'test-lib-lab_search.json', 'example-ci_bench.json')},
                {'lab-engine': core, 'lab-search': search}, active)
    hurt_active = selection in ('leaf-ending-states', 'prepared-turn', 'compact-volatiles') or (
        label == 'candidate' and selection == 'hurt-readers')
    leaf_active = selection == 'leaf-ending-states' and label == 'candidate'
    prepared_active = selection == 'prepared-turn' and label == 'candidate'
    bridge_expected = {LEAF_FEATURE: leaf_active, OBSERVER_FEATURE: False,
                       PREPARED_FEATURE: prepared_active, PREPARED_OBSERVER_FEATURE: False}
    packages = {'lab-engine': ('lib-lab_engine.json', 'test-lib-lab_engine.json')}
    expected = {'lab-engine': {EXPERIMENT_FEATURE: hurt_active, **bridge_expected}}
    if selection in ('leaf-ending-states', 'prepared-turn', 'compact-volatiles'):
        packages['lab-search'] = ('lib-lab_search.json', 'test-lib-lab_search.json', 'example-ci_bench.json')
        expected['lab-search'] = dict(bridge_expected)
    if selection == 'compact-volatiles':
        expected['lab-engine'][COMPACT_FEATURE] = label == 'candidate'
        expected['lab-search'].update({EXPERIMENT_FEATURE: False, COMPACT_FEATURE: False})
    return packages, expected, hurt_active


def preserve_fingerprints(workspace, label, selection):
    packages, expected, hurt_active = fingerprint_expectations(selection, label)
    return preserve_expected_fingerprints(workspace, label, packages, expected, hurt_active)


def validate_strict_feature_closure(package, features, expected):
    """New mode/observer maps declare every known switch and exact package closure."""
    if not set(ALL_EXPERIMENT_FEATURES) <= set(expected):
        return
    if any(name.startswith('experiment-') and name not in expected for name in features):
        raise ValueError('actual compiled feature activation includes an unknown experiment feature')
    required = {name for name, active in expected.items() if active} | PACKAGE_BASE_FEATURES[package]
    if len(features) != len(set(features)) or set(features) != required:
        raise ValueError('actual compiled feature activation differs from exact package closure')


def preserve_expected_fingerprints(workspace, label, packages, expected, hurt_active, profile="release"):
    if profile not in ("release", "debug"): raise ValueError("Unsupported fingerprint profile")
    target = workspace/('target-' + label)
    result = workspace/'ci-results'
    evidence = {'expected_active': hurt_active, 'expected_by_package': expected,
                'feature': EXPERIMENT_FEATURE, 'fingerprints': []}
    if profile != 'release': evidence['profile'] = profile
    fingerprint_root = target/profile/'.fingerprint'
    for package, names in packages.items():
        for directory in sorted(fingerprint_root.glob(package + '-*')):
            for name in names:
                source = directory/name
                if not source.is_file():
                    continue
                destination = result/'fingerprints'/label
                if profile != 'release': destination = destination/profile
                destination = destination/directory.name/name
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
    (result/(f'{label}-features.json' if profile == 'release' else f'{label}-debug-features.json')).write_text(json.dumps(evidence, indent=2)+'\n', encoding='utf-8')
    for package, names in packages.items():
        kinds = {item['kind'] for item in evidence['fingerprints'] if item['package'] == package}
        if kinds != set(names):
            raise ValueError(f'{label}: missing compiled {package} fingerprints: {set(names) - kinds}')
    for item in evidence['fingerprints']:
        validate_strict_feature_closure(item['package'], item['features'], expected[item['package']])
        for feature, active in expected[item['package']].items():
            if (feature in item['features']) != active:
                raise ValueError(f'{label}: actual compiled feature activation differs from request: '
                                 f'{item["target_path"]}: {item["features"]}')
        if (set(item['features']) & set(ALL_EXPERIMENT_FEATURES)) - set(expected[item['package']]):
            raise ValueError(f'{label}: actual compiled feature activation includes an undeclared experiment')
    return evidence


def prepared_validation_command(selection='prepared-turn'):
    if selection in ('all-optimizations', *STRICT_MODES):
        features = (feature_args(selection, 'candidate')[1] + ',lab-search/' + PREPARED_OBSERVER_FEATURE
                    + ',lab-search/' + OBSERVER_FEATURE)
        return ['cargo', 'test', '--locked', '--release', '-p', 'lab-search',
                '--test', 'prepared_turn', '--features', features]
    if selection != 'prepared-turn':
        raise ValueError('Invalid prepared validation selection')
    return ['cargo', 'test', '--locked', '--release', '-p', 'lab-search',
            '--test', 'prepared_turn', '--features',
            'lab-engine/' + EXPERIMENT_FEATURE + ',lab-search/' + PREPARED_OBSERVER_FEATURE]


def validate_prepared_turn(workspace, selection='prepared-turn'):
    """Run observer-dependent differential tests outside both timing targets."""
    result = workspace/'ci-results'
    label = 'prepared-combined-validation' if selection in ('all-optimizations', *STRICT_MODES) else 'prepared-validation'
    target = workspace/('target-' + label)
    if target.exists():
        raise ValueError('Prepared validation target directory must be new')
    command = prepared_validation_command(selection)
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
        expected = {'lab-engine': {EXPERIMENT_FEATURE: True, **bridge}, 'lab-search': dict(bridge)}
        if selection in ('all-optimizations', *STRICT_MODES):
            _, expected, _ = fingerprint_expectations(selection, 'candidate')
            for package in expected:
                expected[package][PREPARED_OBSERVER_FEATURE] = True
                expected[package][OBSERVER_FEATURE] = True
        elif (result/'request.json').is_file() and json.loads((result/'request.json').read_text(encoding='utf-8')).get('candidate_feature') in STRICT_MODES:
            for package in expected:
                expected[package] = {**{name: False for name in ALL_EXPERIMENT_FEATURES}, **expected[package]}
        receipt['selection'] = selection
        receipt['compiler_feature_evidence'] = preserve_expected_fingerprints(
            workspace, label,
            {'lab-engine': ('lib-lab_engine.json',),
             'lab-search': ('lib-lab_search.json', 'test-integration-test-prepared_turn.json')},
            expected, True)
        receipt['status'] = 'ok'
        save()
        return receipt
    except Exception as error:
        receipt.update(status='failed', error=f'{type(error).__name__}: {error}')
        save()
        raise


def observer_runtime_selection(selection, runtime_selection):
    if selection not in NEW_MODES or runtime_selection not in (None, selection, *COMBINED_NEW_MODES):
        raise ValueError('Invalid observer runtime selection')
    return runtime_selection or selection


def new_observer_validation_commands(selection, *, runtime_selection=None):
    runtime_selection = observer_runtime_selection(selection, runtime_selection)
    if selection not in NEW_MODES:
        raise ValueError('Invalid independent observer mode')
    tests = NEW_OBSERVER_TESTS[selection]
    if (not tests or len(set(tests)) != len(tests)
            or any(not re.fullmatch(r'[A-Za-z0-9_:]+', name) for name in tests)):
        raise ValueError(f'{selection}: exact observer test registration must be resolved before launch')
    # This observer gate targets core unit tests. Request the same core feature
    # closure directly, without building a search timing binary with observers.
    _, expected, _ = fingerprint_expectations(runtime_selection, 'candidate')
    features = ','.join('lab-engine/' + name for name, active in expected['lab-engine'].items() if active)
    target = NEW_OBSERVER_TARGETS[selection]
    features += ',' + target['package'] + '/' + NEW_OBSERVERS[selection]
    if target['kind'] == 'test':
        packages = ['-p', target['package']]
        if target.get('additional_package'):
            packages += ['-p', target['additional_package']]
        return [['cargo', 'test', '--locked', '--release', *packages,
                 '--test', target['target'], '--features', features, '--', '--test-threads=1']]
    return [['cargo', 'test', '--locked', '--release', '-p', target['package'], '--lib',
             '--features', features, name, '--', '--exact', '--test-threads=1'] for name in tests]


def validate_new_observer(workspace, selection, *, runtime_selection=None):
    runtime_selection = observer_runtime_selection(selection, runtime_selection)
    commands = new_observer_validation_commands(selection, runtime_selection=runtime_selection)
    result = workspace/'ci-results'
    label = (runtime_selection + '-' if runtime_selection in COMBINED_NEW_MODES else '') + selection + '-observer-validation'
    target = workspace/('target-' + label)
    if os.path.lexists(target):
        raise ValueError('Independent observer target directory must be new')
    receipt_path = result/(label + '.json')
    log_path = result/(label + '.log')
    receipt = {'status': 'running', 'selection': selection, 'runtime_selection': runtime_selection, 'source': 'candidate',
               'commands': commands, 'expected_tests': list(NEW_OBSERVER_TESTS[selection]),
               'target_directory': str(target), 'log': log_path.name,
               'per_command_timeout_seconds': COMMAND_TIMEOUT_SECONDS,
               'cached_results_reused': False, 'performance_measurement': False}

    def save():
        receipt_path.write_text(json.dumps(receipt, indent=2)+'\n', encoding='utf-8')

    save()
    try:
        env = os.environ.copy()
        env['CARGO_TARGET_DIR'] = str(target)
        with log_path.open('w', encoding='utf-8') as log:
            for command in commands:
                log.write('COMMAND '+json.dumps(command)+'\n')
                log.flush()
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
                    raise RuntimeError('Independent observer validation timed out; process group terminated')
                receipt.setdefault('returncodes', []).append(proc.returncode)
                if proc.returncode:
                    raise RuntimeError(f'Independent observer validation failed ({proc.returncode}); see log')
        text = log_path.read_text(encoding='utf-8')
        passed = re.findall(r'^test (\S+) \.\.\. ok$', text, re.MULTILINE)
        summaries = re.findall(r'^test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; '
                               r'(\d+) measured; (\d+) filtered out;', text, re.MULTILINE)
        receipt['passed_tests'] = passed
        target_spec = NEW_OBSERVER_TARGETS[selection]
        counts = target_spec.get('test_counts', (len(NEW_OBSERVER_TESTS[selection]),)) if target_spec['kind'] == 'test' else (1,) * len(commands)
        if (sorted(passed) != sorted(NEW_OBSERVER_TESTS[selection]) or len(summaries) != len(counts)
                or sorted(int(row[0]) for row in summaries) != sorted(counts)
                or any(tuple(map(int, row[1:4])) != (0, 0, 0) for row in summaries)
                or target_spec['kind'] == 'test' and any(int(row[4]) != 0 for row in summaries)):
            raise ValueError('Every exact independent observer test must execute once and pass unskipped')
        _, expected, _ = fingerprint_expectations(runtime_selection, 'candidate')
        core = expected['lab-engine']
        core[NEW_OBSERVERS[selection]] = True
        # cargo test --lib builds the test library, without a separate normal rlib.
        # Require the fingerprint of the executable that actually ran.
        packages = {'lab-engine': ('test-lib-lab_engine.json',)}
        expectations = {'lab-engine': core}
        if target_spec['package'] == 'lab-scenario':
            packages = {'lab-engine': ('lib-lab_engine.json',),
                        'lab-scenario': ('lib-lab_scenario.json',
                                         'test-integration-test-' + target_spec['target'] + '.json')}
            scenario = {name: False for name in ALL_EXPERIMENT_FEATURES}
            scenario[NEW_OBSERVERS[selection]] = True
            expectations['lab-scenario'] = scenario
            if target_spec.get('additional_package') == 'lab-engine':
                packages['lab-engine'] = ('lib-lab_engine.json',
                                          'test-integration-test-' + target_spec['target'] + '.json')
        receipt['compiler_feature_evidence'] = preserve_expected_fingerprints(
            workspace, label, packages, expectations, True)
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
    import dependency_target

    result = workspace/'ci-results'
    request = json.loads((result/'request.json').read_text(encoding='utf-8'))
    selection = request['candidate_feature']
    if selection in STRICT_MODES and request['baseline_sha'] != request['candidate_sha']:
        raise ValueError(f'{selection} requires the same source SHA with features off/on')
    commands_by_version = {label: build_commands(request['suite'], selection, label)
                           for label in ('baseline', 'candidate')}
    expected_args = {label: feature_args(selection, label) for label in commands_by_version}
    if request['feature_args'] != expected_args:
        raise ValueError('Requested feature arguments do not match the explicit selection')
    plan = {'suite': request['suite'], 'candidate_feature': selection,
            'feature_args': expected_args, 'commands_by_version': commands_by_version,
            'per_command_timeout_seconds': COMMAND_TIMEOUT_SECONDS}
    if selection in ('prepared-turn', 'all-optimizations', *STRICT_MODES):
        plan['prepared_validation_command'] = prepared_validation_command()
    if selection in ('all-optimizations', *STRICT_MODES):
        plan['combined_prepared_validation_command'] = prepared_validation_command(selection)
    if selection in NEW_MODES:
        plan['independent_observer_validation_commands'] = new_observer_validation_commands(selection)
    elif selection in COMBINED_NEW_MODES:
        plan['combined_new_observer_validation_commands'] = {
            mode: new_observer_validation_commands(mode, runtime_selection=selection) for mode in NEW_MODES}
    if selection == BORROWED_MODE:
        plan['borrowed_child_keys_validation'] = 'fresh common5 dense/compact named tests, allocator, exact OFF/ON records and public search activation'
    if selection == MATRIX_MODE:
        plan['matrix_pass_through_validation'] = 'fresh common5 dense/compact bit-exact matrix, allocator and public search activation proof'
    (result/'test-plan.json').write_text(json.dumps(plan, indent=2)+'\n', encoding='utf-8')
    provenance = json.loads((result/'provenance.json').read_text(encoding='utf-8'))
    provenance['build_plan'] = plan
    provenance['compiler_feature_evidence'] = {}
    (result/'provenance.json').write_text(json.dumps(provenance, indent=2)+'\n', encoding='utf-8')
    for label in ('baseline', 'candidate'):
        env = os.environ.copy()
        target = workspace/('target-'+label)
        dependency_seeded = dependency_target.prepare_target(workspace, label)
        env['CARGO_TARGET_DIR'] = str(target)
        reused = build_cache.restore(workspace, label)
        receipt = {'schema_version': 1, 'status': 'running', 'label': label,
                   'suite': request['suite'], 'selection': selection, 'reused': reused,
                   'dependency_seeded': dependency_seeded,
                   'commands': [], 'log': label + '-build.log',
                   'feature_evidence': label + '-features.json'}
        receipt_path = result/(label + '-build-receipt.json')
        timings_before = timing_snapshot(target) if not reused else {}
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
        finally:
            if not reused:
                try:
                    receipt['cargo_timings'] = preserve_build_timings(workspace, label, timings_before)
                except OSError as error:
                    # Report artifact failures without replacing a Cargo error or retrying it.
                    receipt['cargo_timings_error'] = f'{type(error).__name__}: {error}'
                receipt_path.write_text(json.dumps(receipt, indent=2)+'\n', encoding='utf-8')
        if not reused:
            build_cache.seal(workspace, label, provenance['compiler_feature_evidence'][label])
        provenance.setdefault('build_cache', {})[label] = json.loads(
            (result/('cache-' + label + '.json')).read_text(encoding='utf-8'))
        (result/'provenance.json').write_text(json.dumps(provenance, indent=2)+'\n', encoding='utf-8')
    if selection in ('prepared-turn', 'all-optimizations', *STRICT_MODES):
        provenance['prepared_validation'] = validate_prepared_turn(workspace)
        (result/'provenance.json').write_text(json.dumps(provenance, indent=2)+'\n', encoding='utf-8')
    if selection in ('all-optimizations', *STRICT_MODES):
        provenance['combined_prepared_validation'] = validate_prepared_turn(workspace, selection)
        (result/'provenance.json').write_text(json.dumps(provenance, indent=2)+'\n', encoding='utf-8')
    if selection in NEW_MODES:
        provenance['independent_observer_validation'] = validate_new_observer(workspace, selection)
        (result/'provenance.json').write_text(json.dumps(provenance, indent=2)+'\n', encoding='utf-8')
    elif selection in COMBINED_NEW_MODES:
        provenance['combined_new_observer_validation'] = {}
        for mode in NEW_MODES:
            provenance['combined_new_observer_validation'][mode] = validate_new_observer(
                workspace, mode, runtime_selection=selection)
            (result/'provenance.json').write_text(json.dumps(provenance, indent=2)+'\n', encoding='utf-8')

    if selection == BORROWED_MODE:
        import borrowed_child_keys
        provenance['borrowed_child_keys_validation'] = borrowed_child_keys.validate(workspace)
        (result/'provenance.json').write_text(json.dumps(provenance, indent=2)+'\n', encoding='utf-8')

    if selection == MATRIX_MODE:
        import matrix_pass_through
        provenance['matrix_pass_through_validation'] = matrix_pass_through.validate(workspace)
        (result/'provenance.json').write_text(json.dumps(provenance, indent=2)+'\n', encoding='utf-8')

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('stage', choices=('refs', 'prepare', 'verify-prepared', 'build'))
    parser.add_argument('--workspace', type=Path, required=True)
    args = parser.parse_args()
    try:
        globals()[args.stage.replace('-', '_')](args.workspace.resolve())
    except Exception as error:
        result = args.workspace/'ci-results'
        result.mkdir(exist_ok=True)
        (result/(args.stage+'-error.txt')).write_text(f'{type(error).__name__}: {error}\n', encoding='utf-8')
        raise


if __name__ == '__main__':
    main()
