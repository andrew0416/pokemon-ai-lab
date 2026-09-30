"""Fresh Linux P10 representation gate. No benchmark timing or target reuse."""
import argparse
from collections import Counter
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import signal
import subprocess

import ci

PROBE_SHA256 = '80290baa93490ad101e4928b6948a1cac40187b54a0971324ad054047a3bf0fb'
PROBE_SOURCE = 'engine/scenario/examples/ci_compact_probe.rs'
PROBE_EXAMPLE = 'ci_compact_probe'
HURT = 'experiment-hurt-readers'
COMPACT = 'experiment-compact-volatiles'
OFF_GUARD = 'volatile::tests::default_storage_keeps_copy_and_public_tuple_api'
KILL_SIGNAL = getattr(signal, 'SIGKILL', 9)  # Windows hosts only exercise mocked Linux commands.
KINDS = {'registry-singletons': 8, 'volatiles': 26, 'clone-independence': 6,
         'state-instructions': 2, 'hidden-inactive': 3, 'fixture': 10, 'complete': 1}
FIELDS = {
    'registry-singletons': {'kind', 'pattern', 'records'},
    'volatiles': {'kind', 'label', 'active_indices', 'entries', 'hash_events', 'is_empty', 'key_hash'},
    'clone-independence': {'kind', 'label', 'destination_counts', 'source_entries'},
    'state-instructions': {'kind', 'instructions', 'populated', 'replaced', 'restored', 'slots_per_side', 'switched'},
    'hidden-inactive': {'kind', 'canonical_hidden', 'non_none_count', 'state'},
    'fixture': {'kind', 'choice_normalization', 'engine_choices', 'factored', 'fixture', 'input_choices', 'records', 'rolls'},
    'complete': {'kind', 'fixture_configurations', 'registry_count', 'schema_version', 'scope', 'singleton_patterns'},
}
LAYOUT_NUMBERS = ('volatile_state', 'volatiles', 'volatiles_align', 'slot', 'side1',
                  'side2', 'state1', 'state2', 'instruction')
VARIANTS = (('baseline', 'baseline', 'compact-probe-baseline', False),
            ('candidate-on', 'candidate', 'compact-probe-candidate', True),
            ('candidate-off', 'candidate', 'compact-off', False))


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f'Duplicate JSON key: {key}')
        result[key] = value
    return result


def _json(value):
    def reject_constant(value):
        raise ValueError(f'Non-finite JSON number: {value}')
    return json.loads(value, object_pairs_hook=_object, parse_constant=reject_constant)


def validate_output(path):
    raw = path.read_bytes()
    if not raw.endswith(b'\n') or b'\r' in raw:
        raise ValueError('Probe must emit complete LF-terminated JSONL')
    lines = raw.decode('utf-8').splitlines()
    if len(lines) != 56 or any(not line for line in lines):
        raise ValueError('Probe must emit exactly 56 nonempty JSONL records')
    rows = [_json(line) for line in lines]
    if any(not isinstance(row, dict) or row.get('kind') not in FIELDS
           or set(row) != FIELDS[row['kind']] for row in rows):
        raise ValueError('Probe JSONL has an unknown or malformed record')
    counts = Counter(row['kind'] for row in rows)
    if counts != KINDS or rows[-1]['kind'] != 'complete':
        raise ValueError('Probe record counts or completion order differ')
    final = rows[-1]
    if any(type(final[key]) is not int or final[key] != value for key, value in
           {'schema_version': 1, 'registry_count': 112, 'singleton_patterns': 8,
            'fixture_configurations': 10}.items()):
        raise ValueError('Probe completion contract differs')
    return {'records': len(rows), 'kind_counts': dict(counts), 'sha256': sha(path), 'bytes': len(raw)}


def validate_layout(path):
    raw = path.read_bytes()
    if not raw.endswith(b'\n') or len(raw.splitlines()) != 1:
        raise ValueError('Layout must be one complete JSON line')
    value = _json(raw)
    if (not isinstance(value, dict) or value.get('kind') != 'layout'
            or type(value.get('schema_version')) is not int or value['schema_version'] != 1
            or set(value) != {'kind', 'schema_version', 'scope', *LAYOUT_NUMBERS}
            or not isinstance(value['scope'], str)
            or any(type(value[name]) is not int or value[name] <= 0 for name in LAYOUT_NUMBERS)):
        raise ValueError('Malformed static layout record')
    return value


def validate_off_guard(path):
    text = path.read_text(encoding='utf-8')
    passed = re.findall(r'^test (\S+) \.\.\. ok$', text, re.M)
    summaries = re.findall(r'^test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; '
                           r'(\d+) measured; (\d+) filtered out;', text, re.M)
    if passed.count(OFF_GUARD) != 1 or len(summaries) != 1:
        raise ValueError('Default-off Copy/public tuple guard must execute exactly once')
    counts = tuple(map(int, summaries[0]))
    if counts[0] != len(passed) or not counts[0] or any(counts[1:]):
        raise ValueError('Default-off core gate must pass without ignored or filtered tests')
    return {'guard': OFF_GUARD, 'guard_passes': 1, 'passed': counts[0],
            'failed': 0, 'ignored': 0, 'filtered': 0}


def environment(target):
    if platform.system() != 'Linux' or platform.machine() not in ('x86_64', 'AMD64'):
        raise ValueError('Compact gate requires Linux x86_64')
    if os.environ.get('RUSTFLAGS') != '-Ctarget-cpu=x86-64':
        raise ValueError('Compact gate requires explicit generic x86-64 RUSTFLAGS')
    if any(os.environ.get(name) for name in ('CARGO_ENCODED_RUSTFLAGS', 'RUSTC_WRAPPER',
            'RUSTC_WORKSPACE_WRAPPER', 'CARGO_BUILD_TARGET', 'RUSTDOCFLAGS')):
        raise ValueError('Custom target, compiler wrappers, or encoded/doc flags are not supported')
    env = os.environ.copy()
    env.update(CARGO_TARGET_DIR=str(target), CARGO_BUILD_JOBS='1', CARGO_INCREMENTAL='0',
               CARGO_PROFILE_RELEASE_OPT_LEVEL='3', CARGO_PROFILE_RELEASE_DEBUG='1',
               CARGO_PROFILE_RELEASE_LTO='off', CARGO_PROFILE_RELEASE_CODEGEN_UNITS='16',
               RUST_MIN_STACK='16777216', LAB_ENGINE_FACTORED='0')
    return env


def _configuration(env):
    keys = ('CARGO_TARGET_DIR', 'CARGO_BUILD_JOBS', 'CARGO_INCREMENTAL', 'RUSTFLAGS',
            'RUST_MIN_STACK', 'LAB_ENGINE_FACTORED')
    return {key: value for key, value in sorted(env.items())
            if key in keys or key.startswith('CARGO_PROFILE_RELEASE_')}


def _source_hashes(root):
    hashes = {}
    for folder in ('engine', 'teams'):
        for path in sorted((root/folder).rglob('*')):
            if path.is_symlink():
                raise ValueError(f'Source links not supported: {path}')
            if path.is_file():
                hashes[path.relative_to(root).as_posix()] = sha(path)
    return hashes


def _run(argv, cwd, env, stem, timeout, commands, save):
    item = {'argv': argv, 'cwd': str(cwd), 'configuration': _configuration(env),
            'timeout_seconds': timeout, 'returncode': None, 'status': 'running',
            'stdout': str(stem.with_suffix('.stdout')), 'stderr': str(stem.with_suffix('.stderr'))}
    commands.append(item)
    stem.parent.mkdir(parents=True, exist_ok=True)
    save()
    try:
        with stem.with_suffix('.stdout').open('wb') as stdout, stem.with_suffix('.stderr').open('wb') as stderr:
            process = subprocess.Popen(argv, cwd=cwd, env=env, stdout=stdout, stderr=stderr,
                                       start_new_session=True)
            try:
                process.wait(timeout=timeout)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, KILL_SIGNAL)
                process.wait()
                item['returncode'] = process.returncode
                raise RuntimeError(f'Compact gate command timed out: {argv}')
            item['returncode'] = process.returncode
        if process.returncode:
            raise RuntimeError(f'Compact gate command failed ({process.returncode}): {argv}')
        item['status'] = 'success'
    except Exception as error:
        item.update(status='failed', error=f'{type(error).__name__}: {error}')
        raise
    finally:
        for stream in ('stdout', 'stderr'):
            path = Path(item[stream])
            if path.exists():
                item[stream + '_sha256'] = sha(path)
                item[stream + '_bytes'] = path.stat().st_size
        save()
    return item


def _fingerprints(workspace, label, compact, off_guard):
    request = json.loads((workspace/'ci-results/request.json').read_text(encoding='utf-8'))
    known = ci.ALL_EXPERIMENT_FEATURES if request.get('candidate_feature') in ci.STRICT_MODES else ci.EXPERIMENT_FEATURES
    expected = {feature: False for feature in known}
    expected.update({HURT: True, COMPACT: compact})
    kinds = ['lib-lab_engine.json']
    if off_guard:
        kinds.append('test-lib-lab_engine.json')
    packages = {'lab-engine': tuple(kinds),
                'lab-scenario': ('lib-lab_scenario.json', 'example-' + PROBE_EXAMPLE + '.json')}
    return ci.preserve_expected_fingerprints(workspace, label, packages,
        {'lab-engine': expected, 'lab-scenario': {key: False for key in expected}}, True)


def slot_diff_semantic_output(path, selection):
    """Only P8e may change emitted outcome instruction representation.

    Keep explicit state-instructions cases, every complete before/after state,
    probabilities, hidden diagnostics, suspension, record/ending order and all
    other fields. The Rust probe itself still applies/reverses every instruction
    and asserts complete state restoration and incremental-hash consistency.
    """
    if selection not in ('slot-diff', *ci.COMBINED_NEW_MODES):
        raise ValueError('Instruction representation exemption is only valid for slot-diff or explicit combined P8d/e/f modes')
    validate_output(path)
    rows = [_json(line) for line in path.read_text(encoding='utf-8').splitlines()]
    removed = 0
    for row in rows:
        if row['kind'] != 'fixture':
            continue
        if not isinstance(row['records'], list):
            raise ValueError('Fixture records must remain an ordered list')
        for record in row['records']:
            if not isinstance(record, dict) or not isinstance(record.get('endings'), list):
                raise ValueError('Every fixture record must retain ordered endings')
            for ending in record['endings']:
                if not isinstance(ending, dict) or not isinstance(ending.get('instructions'), str):
                    raise ValueError('Missing explicit outcome instruction representation')
                del ending['instructions']
                removed += 1
    if not removed:
        raise ValueError('No complete outcome instruction representations were inspected')
    canonical = json.dumps(rows, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode('utf-8')
    return {'semantic_sha256': hashlib.sha256(canonical).hexdigest(),
            'removed_outcome_instruction_fields': removed,
            'excluded_path': 'fixture.records[].endings[].instructions'}


def validate_independent_candidate(workspace, out_dir, selection, receipt, save):
    if selection not in ci.STRICT_MODES:
        raise ValueError('Invalid independent candidate representation comparison')
    comparison = {'selection': selection, 'variants': {}, 'performance_measurement': False,
                  'scope': ('all four validated runtime flags and P8d common; candidate adds only P11; exact raw output'
                            if selection == ci.INLINE_MODE else
                            'all four validated runtime flags and P8d common; candidate adds only P8e/P8f'
                            if selection == ci.P8D_VS_P8DEF else
                            'all four validated runtime flags common; all three new runtime flags on candidate'
                            if selection == ci.P8DEF_COMBINED else
                            'all four validated core runtime flags common; exactly one new candidate flag'),
                  'raw_outputs_retained': True}
    receipt['independent_candidate_comparison'] = comparison
    for tree in ('baseline', 'candidate'):
        if os.path.lexists(workspace/('target-' + selection + '-probe-' + tree)):
            raise ValueError('Independent representation targets must be fresh')
    for tree in ('baseline', 'candidate'):
        label = selection + '-probe-' + tree
        target = workspace/('target-' + label)
        env = environment(target)
        cwd = workspace/tree/'engine'
        _, expected, _ = ci.fingerprint_expectations(selection, tree)
        core_expected = expected['lab-engine']
        feature = ','.join('lab-engine/' + name for name, active in core_expected.items() if active)
        item = {'tree': tree, 'target': str(target), 'features': feature,
                'configuration': _configuration(env)}
        comparison['variants'][tree] = item
        folder = out_dir/'independent-candidate'/tree
        _run(['cargo', 'build', '--locked', '--release', '-p', 'lab-scenario', '--example',
              PROBE_EXAMPLE, '--features', feature], cwd, env, folder/'build', 1800, receipt['commands'], save)
        item['compiler_feature_evidence'] = ci.preserve_expected_fingerprints(
            workspace, label,
            {'lab-engine': ('lib-lab_engine.json',),
             'lab-scenario': ('lib-lab_scenario.json', 'example-' + PROBE_EXAMPLE + '.json')},
            {'lab-engine': core_expected,
             'lab-scenario': {name: False for name in ci.ALL_EXPERIMENT_FEATURES}}, True)
        binary = target/'release/examples'/PROBE_EXAMPLE
        if binary.is_symlink() or not binary.is_file() or not os.access(binary, os.X_OK):
            raise ValueError('Independent representation probe must be a regular executable')
        item['binary_sha256'] = sha(binary)
        _run([str(binary), str(workspace/'baseline/engine')], cwd, env, folder/'probe',
             600, receipt['commands'], save)
        item['output'] = validate_output(folder/'probe.stdout')
        if selection in ('slot-diff', *ci.COMBINED_NEW_MODES):
            item['semantic_output'] = slot_diff_semantic_output(folder/'probe.stdout', selection)
        _run([str(binary), '--layout'], cwd, env, folder/'layout', 60, receipt['commands'], save)
        item['layout'] = validate_layout(folder/'layout.stdout')
        save()
    before = (out_dir/'independent-candidate/baseline/probe.stdout').read_bytes()
    after = (out_dir/'independent-candidate/candidate/probe.stdout').read_bytes()
    comparison['complete_jsonl_byte_equal'] = before == after
    if selection in ('slot-diff', *ci.COMBINED_NEW_MODES):
        outputs = [comparison['variants'][tree]['semantic_output'] for tree in ('baseline', 'candidate')]
        if outputs[0] != outputs[1]:
            raise ValueError('Slot-diff complete semantic probe outputs differ beyond instruction representation')
        comparison.update(semantic_equal=True,
                          excluded_path='fixture.records[].endings[].instructions')
    elif before != after:
        raise ValueError('Independent candidate complete 56-record outputs must be byte-identical')
    else:
        comparison['semantic_equal'] = True
    comparison['status'] = 'success'
    save()


def validate_compact(workspace, out_dir):
    workspace = Path(workspace).resolve(strict=True)
    out_dir = Path(out_dir).resolve()
    if not out_dir.is_relative_to(workspace/'ci-results') or out_dir == workspace/'ci-results':
        raise ValueError('Compact evidence must be a new directory inside ci-results')
    out_dir.mkdir(parents=True, exist_ok=False)
    receipt = {'schema_version': 1, 'status': 'running', 'performance_measurement': False,
               'probe_source': PROBE_SOURCE, 'probe_sha256': PROBE_SHA256,
               'scope': 'bounded representation/fixture equivalence; fresh targets; static layout separate',
               'variants': {}, 'commands': []}
    def save():
        (out_dir/'receipt.json').write_text(json.dumps(receipt, indent=2)+'\n', encoding='utf-8')
    save()
    try:
        request = json.loads((workspace/'ci-results/request.json').read_text(encoding='utf-8'))
        if request['candidate_feature'] not in ('compact-volatiles', 'all-optimizations', *ci.STRICT_MODES):
            raise ValueError('Compact gate requires compact-volatiles or all-optimizations mode')
        receipt['selection'] = request['candidate_feature']
        receipt['isolated_feature_scope'] = 'hurt-readers common; compact on/off; leaf/prepared/observers off'
        ci.verify_prepared(workspace)
        receipt['prepared_source_verification_sha256'] = sha(workspace/'ci-results/prepared-source-verification.json')
        provenance_path = workspace/'ci-results/provenance.json'
        provenance = json.loads(provenance_path.read_text(encoding='utf-8'))
        receipt['prepared_provenance_sha256'] = sha(provenance_path)
        receipt['toolchain'] = {name: provenance[name] for name in ('rustc', 'cargo')}
        if sha(Path(__file__).with_suffix('.rs')) != PROBE_SHA256:
            raise ValueError('Controller probe differs from the frozen P10 probe')
        sources = {}
        for tree in ('baseline', 'candidate'):
            probe = workspace/tree/PROBE_SOURCE
            if probe.is_symlink() or sha(probe) != PROBE_SHA256:
                raise ValueError(f'{tree}: injected probe differs from the frozen P10 probe')
            sources[tree] = _source_hashes(workspace/tree)
        (out_dir/'source-hashes.json').write_text(json.dumps(sources, indent=2)+'\n', encoding='utf-8')
        receipt['source_hashes_sha256'] = sha(out_dir/'source-hashes.json')
        receipt['requested_sources'] = {label: request[label + '_sha'] for label in sources}
        # All three paths must be absent before the first command; never restore cache here.
        for _, _, label, _ in VARIANTS:
            target = workspace/('target-' + label)
            if os.path.lexists(target):
                raise ValueError(f'Compact target must be fresh: {target}')
            environment(target)
        if request['candidate_feature'] in ci.STRICT_MODES:
            for tree in ('baseline', 'candidate'):
                if os.path.lexists(workspace/('target-' + request['candidate_feature'] + '-probe-' + tree)):
                    raise ValueError('Independent representation targets must be fresh')
        for variant, tree, label, compact in VARIANTS:
            target = workspace/('target-' + label)
            env = environment(target)
            cwd = workspace/tree/'engine'
            feature = 'lab-engine/' + HURT + (',lab-engine/' + COMPACT if compact else '')
            item = {'tree': tree, 'target': str(target), 'features': feature,
                    'configuration': _configuration(env)}
            receipt['variants'][variant] = item
            if variant == 'candidate-off':
                argv = ['cargo', 'test', '--locked', '--release', '-p', 'lab-engine', '--lib',
                        '--features', feature, '--', '--test-threads=1']
                _run(argv, cwd, env, out_dir/variant/'core-tests', 1800, receipt['commands'], save)
                item['off_api_guard'] = validate_off_guard(out_dir/variant/'core-tests.stdout')
            argv = ['cargo', 'build', '--locked', '--release', '-p', 'lab-scenario',
                    '--example', PROBE_EXAMPLE, '--features', feature]
            _run(argv, cwd, env, out_dir/variant/'build', 1800, receipt['commands'], save)
            item['compiler_feature_evidence'] = _fingerprints(workspace, label, compact, variant == 'candidate-off')
            binary = target/'release/examples'/PROBE_EXAMPLE
            if binary.is_symlink() or not binary.is_file() or not os.access(binary, os.X_OK):
                raise ValueError(f'Probe is not a regular executable: {binary}')
            item['binary_sha256'] = sha(binary)
            # Every binary reads the SAME baseline fixture/team tree.
            _run([str(binary), str(workspace/'baseline/engine')], cwd, env,
                 out_dir/variant/'probe', 600, receipt['commands'], save)
            item['output'] = validate_output(out_dir/variant/'probe.stdout')
            _run([str(binary), '--layout'], cwd, env, out_dir/variant/'layout',
                 60, receipt['commands'], save)
            item['layout'] = validate_layout(out_dir/variant/'layout.stdout')
            save()
        raw = [(out_dir/variant/'probe.stdout').read_bytes() for variant, *_ in VARIANTS]
        if raw[0] != raw[1] or raw[0] != raw[2]:
            raise ValueError('All three complete 56-record probe outputs must be byte-identical')
        variants = receipt['variants']
        if variants['baseline']['layout'] != variants['candidate-off']['layout']:
            raise ValueError('Default-off layout differs from baseline')
        if variants['baseline']['layout'] == variants['candidate-on']['layout']:
            raise ValueError('Compact-on layout did not differ despite requested activation')
        if request['candidate_feature'] in ci.STRICT_MODES:
            validate_independent_candidate(workspace, out_dir, request['candidate_feature'], receipt, save)
        if sources != {tree: _source_hashes(workspace/tree) for tree in sources}:
            raise ValueError('Sources or fixture inputs changed during compact validation')
        receipt.update(status='success', complete_jsonl_byte_equal=True, source_inputs_unchanged=True,
                       default_off_layout_equal=True, compact_layout_distinct=True)
    except Exception as error:
        receipt.update(status='failed', error=f'{type(error).__name__}: {error}')
        raise
    finally:
        save()
    return receipt


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--workspace', type=Path, required=True)
    parser.add_argument('--out-dir', type=Path, required=True)
    args = parser.parse_args()
    validate_compact(args.workspace, args.out_dir)


if __name__ == '__main__':
    main()
