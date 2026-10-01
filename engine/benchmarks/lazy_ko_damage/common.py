"""Frozen source/test/corpus contracts for the isolated P1e candidate comparison."""
from pathlib import Path
import copy
import importlib.util
import math
import re
import statistics
import sys

HERE = Path(__file__).resolve().parent
INHERITED = HERE.parent / 'turn_distribution'
sys.path.append(str(INHERITED))
import contract as c
spec = importlib.util.spec_from_file_location('inherited_distribution_ci', INHERITED / 'ci.py')
base_ci = importlib.util.module_from_spec(spec)
spec.loader.exec_module(base_ci)

SOURCE_PARENT = 'a4a881420ca35335c6e0ac4c73fd9e686dd617c0'
FEATURE = 'experiment-lazy-ko-damage'
BENCHMARK = 'engine/scenario/src/bin/lab-distribution-bench.rs'
BENCHMARK_SHA = '8b04912f6494a3617d8956299ae6cfb9191221d46027e6d2c46df72c9d588e1a'
PLAN_SHA = {
    'opening-0000': 'bbd7c02f87855ab7d0790cd4146c1eeee8885ae00ca5da5ef26a9cf14559ea0f',
    'opening-0001': 'e000643e06333b644be1a6f5056f8f7435c9bd5d925bf6ffd30f7011f08eda9d',
    'opening-0429': '2a6ec43ac6e821e1d710a94542f80f6ab04f37416406fb6a932b40f3d6de89ec',
}
CONTROL_CASES = ('opening-0000', 'opening-0001')
TAIL_CASE = 'opening-0429'
PAIR_COUNT = 3
PUBLIC_TARGET = 'lazy_ko_damage'
FACTORED_TESTS = (
    'hp_fixtures_match_through_the_factored_path',
    'heavy_turns_match_through_the_factored_path',
    'spread_damage_reduced_rolls_match_the_flat_enumeration',
)
SKIP_FACTORED = ('spread_damage_full_matches_sampling', 'max_support_bounds_the_distance_from_the_exact_distribution')

def binding(path=None):
    value = c.strict_json((path or HERE / 'source-binding.json').read_bytes())
    c.require(re.fullmatch('[0-9a-f]{40}', value['source_sha']) is not None, 'P1e source is not frozen')
    c.require(value['source_parent'] == SOURCE_PARENT and value['benchmark_sha256'] == BENCHMARK_SHA,
              'Wrong P1e parent or inherited benchmark')
    c.require(value['feature'] == FEATURE and value['public_target'] == PUBLIC_TARGET, 'Feature/test target contract changed')
    c.require(value['source_ref'].startswith('refs/heads/codex/'), 'Unbound source ref')
    pins = value['changed_file_sha256']
    c.require(isinstance(pins, dict) and pins and all(
        (name.startswith('engine/core/') or name in ('engine/scenario/Cargo.toml', 'engine/scenario/tests/lazy_ko_damage.rs'))
        and '..' not in name.split('/') and re.fullmatch('[0-9a-f]{64}', digest)
        for name, digest in pins.items()), 'Unbound or out-of-scope source changes')
    c.require('engine/core/Cargo.toml' in pins and 'engine/scenario/tests/lazy_ko_damage.rs' in pins,
              'Missing implementation or public regression source')
    suites = value['core_suites']
    c.require(isinstance(suites, list) and suites, 'Core test bindings missing')
    labels = set()
    for suite in suites:
        c.fields(suite, 'label filter arms tests')
        c.require(re.fullmatch('[a-z][a-z0-9_]+', suite['label']) is not None and suite['label'] not in labels,
                  'Invalid/duplicate suite label')
        labels.add(suite['label'])
        c.require(suite['arms'] in (['on'], ['off', 'on']) and isinstance(suite['filter'], str)
                  and suite['filter'].startswith('turn::') and 'UNBOUND' not in suite['filter'], 'Core suite unbound')
        named_tests(suite['tests'])
    named_tests(value['public_tests'])
    for field in ('public_comparator_sha256', 'root_source_review_sha256', 'logic_receipt_sha256'):
        c.require(re.fullmatch('[0-9a-f]{64}', value[field]) is not None, 'Unbound review/comparison gate: ' + field)
    c.require(value['public_comparator_sha256'] == c.sha(HERE / 'compare_public.py'), 'Public comparator changed')
    c.require(isinstance(value['public_record_contract'], dict) and value['public_record_contract'], 'Unbound public record schema')
    return value

def named_tests(names):
    c.require(isinstance(names, list) and names and len(names) == len(set(names))
              and all(isinstance(name, str) and re.fullmatch('[A-Za-z0-9_:]+', name) for name in names),
              'Exact nonempty unique named tests required')

def features(enabled):
    return list(c.CORE_FEATURES) + ([FEATURE] if enabled else [])

def feature_args(enabled):
    return ['--features', ','.join('lab-engine/' + value for value in features(enabled))]

def fixed_case(workspace, case_id):
    c.require(case_id in PLAN_SHA, 'Unapproved measurement case')
    manifest = c.corpus(workspace / 'controller')
    case = manifest['cases'][int(case_id.rsplit('-', 1)[1])]
    c.require(case['id'] == case_id, 'Case order differs')
    path = c.safe_file(workspace / 'controller', 'engine/benchmarks/lazy_ko_damage/' + case_id + '.plan.json')
    c.require(c.sha(path) == PLAN_SHA[case_id], 'Frozen opening plan changed')
    c.description(c.line(path), case)
    return case, path

def comparison(left, right, plan, case):
    """Compare emitted scalar metrics; public state/support tests are a separate gate.

    Different exact factorizations can have different component counts and upper bounds.
    Benchmark JSON does not emit sampled states, so this must never claim state equality.
    """
    c.result(left, plan, case)
    c.result(right, plan, case)
    exact_top = ('schema', 'status', 'description', 'metric_schema', 'all_state_restored',
                 'all_sample_outcomes_in_reference', 'timing_scope', 'suspension_scope',
                 'sample_policy', 'probability_policy', 'execution_policy')
    for key in exact_top:
        c.require(left[key] == right[key], 'Measurement identity changed: ' + key)
    deltas = {}
    def near(a, b, key):
        c.require(c.number(a) and c.number(b) and abs(a - b) <= 1e-9, 'Derived metric drift: ' + key)
        deltas[key] = abs(a - b)
    for key in ('method', 'tv_bound', 'full_support_materialized'):
        c.require(left['reference'][key] == right['reference'][key], 'Reference method changed')
    near(left['reference']['total_mass'], right['reference']['total_mass'], 'reference.total_mass')
    for key, value in left['non_hp_state_posthoc_top32'].items():
        other = right['non_hp_state_posthoc_top32'][key]
        if key in ('retained_mass', 'omitted_mass', 'renormalized_tv'):
            near(value, other, 'top32.' + key)
        else:
            c.require(value == other, 'Posthoc projection changed: ' + key)
    for index, (a, b) in enumerate(zip(left['samples'], right['samples'])):
        for key in ('count', 'seed', 'raw_outcomes', 'unique_full_states'):
            c.require(a[key] == b[key], 'Sample identity/count changed: ' + key)
        near(a['sample_total_mass'], b['sample_total_mass'], f'sample.{index}.mass')
        for projection in ('full_state', 'non_hp_state'):
            for key in ('coverage', 'tv', 'outside_reference_mass'):
                near(a[projection][key], b[projection][key], f'sample.{index}.{projection}.{key}')
            c.require(a[projection]['unique_states'] == b[projection]['unique_states'], 'Sample support count changed')
    return {
        'passed': True, 'derived_metric_tolerance': 1e-9,
        'largest_derived_metric_delta': max(deltas.values(), default=0),
        'scope': 'Frozen input/actions/seeds exact; emitted mass and derived metrics within 1e-9; sampled outcome counts exact. Benchmark JSON does not emit sampled states or the full reference distribution.',
        'full_output_state_or_sampled_state_equality_proven': False,
        'factorization_fields_retained_but_not_equality_gated': ['components', 'flat_count_upper_bound', 'suspended_components'],
        'reference_components': {'baseline': left['reference']['components'], 'candidate': right['reference']['components']},
    }

def timed_pair(off, on, plan, case):
    c.require(off['status'] == on['status'] == 'ok', 'Only complete paired cases have kernel ratios')
    proof = comparison(off['result'], on['result'], plan, case)
    left = off['result']['reference']['kernel_ns']
    right = on['result']['reference']['kernel_ns']
    return {'comparison': proof, 'baseline_reference_kernel_ns': left, 'candidate_reference_kernel_ns': right,
            'candidate_over_baseline': right / left, 'reduction_percent': 100 * (1 - right / left)}

def tail_comparison(off, on, plan, case):
    statuses = {arm: row['status'] for arm, row in (('baseline', off), ('candidate', on))}
    c.require(set(statuses.values()) <= {'ok', 'timeout'}, 'Tail execution/validation failed')
    if statuses == {'baseline': 'ok', 'candidate': 'ok'}:
        return dict(status='complete_pair', censored=False, kernel_ratio=timed_pair(off, on, plan, case),
                    full500_complete=False, official_full500_metrics=None)
    # Even a timeout is whole-child timeout: do not divide it by API kernel_ns.
    # The matched completed whole-child time gives at most a process-time bound.
    bound = None
    if statuses['baseline'] == 'timeout' and statuses['candidate'] == 'ok':
        boundary = off['measurement']['process']['timeout_seconds']
        completed = on['measurement']['process']['wall_seconds']
        c.require(c.number(completed) and completed > 0, 'Invalid completed wall time')
        bound = {'baseline_over_candidate_process_wall_lower_bound': boundary / completed,
                 'scope': 'Whole measurement child including reference, all samples and metric preparation; not an API kernel speed ratio. Scheduling and termination are external timing limitations.'}
    return {'status': 'censored', 'censored': True, 'arm_status': statuses,
            'kernel_ratio': None, 'completed_pair_speedup': None, 'whole_process_bound': bound,
            'full500_complete': False, 'official_full500_metrics': None}

def control_summary(pairs):
    c.require(len(pairs) == len(CONTROL_CASES) * PAIR_COUNT, 'Missing paired controls')
    expected = {(case, repeat) for repeat in range(PAIR_COUNT) for case in CONTROL_CASES}
    c.require({(row['case_id'], row['repeat']) for row in pairs} == expected, 'Changed/duplicate pair identities')
    by_case = {}
    for case in CONTROL_CASES:
        rows = [row['timing'] for row in pairs if row['case_id'] == case]
        ratios = [row['candidate_over_baseline'] for row in rows]
        by_case[case] = {'pairs': PAIR_COUNT, 'candidate_over_baseline_ratios': ratios,
                         'median_candidate_over_baseline': statistics.median(ratios),
                         'median_reduction_percent': 100 * (1 - statistics.median(ratios)),
                         'baseline_reference_kernel_ns': [row['baseline_reference_kernel_ns'] for row in rows],
                         'candidate_reference_kernel_ns': [row['candidate_reference_kernel_ns'] for row in rows]}
    return {'by_case': by_case, 'baseline_arm': 'original', 'candidate_arm': 'on',
            'scope': 'Two fixed completed pilot cases, three rotated original/OFF/ON trials each; primary API reference kernel ratio is original versus ON, no statistical significance or overall engine improvement claim',
            'official_full500_metrics': None, 'full500_complete': False, 'warmup': 'No separate warmup; all three pairs retained'}
