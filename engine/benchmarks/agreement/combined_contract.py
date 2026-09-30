"""Fail-closed combined experiment configuration and denominator checks.

This adds experiment scope requirements to the existing agreement comparator;
it does not relax fixture legality, stream equality, or oracle semantics.
"""
from collections import Counter
import argparse
import json
from pathlib import Path
import re

FEATURES = {
    'lab-engine/experiment-hurt-readers',
    'lab-engine/experiment-compact-volatiles',
    'lab-search/experiment-leaf-ending-observer',
    'lab-search/experiment-prepared-turn-observe',
}
COUNTS = {'oracle': 3056, 'turn': 2808, 'search': 320}
TOTAL = sum(COUNTS.values())
ACTIVATION_SUITE = 'prepared-leaf-nonleaf-v1'
ACTIVATION_IDS = [f'{mode}-side{side}-{chance}' for mode in ('exact', 'mixed')
                  for side in (1, 2) for chance in ('expect', 'worst')]
ACTIVATION_CONFIG = {'depth': 2, 'threads': 1, 'rolls': 'Median', 'factored': False,
                     'fixture': 'harden-protect-toy-v1'}
ACTIVATION_FEATURES = {'prepared_compiled', 'prepared_observer_compiled',
                       'leaf_compiled', 'leaf_observer_compiled'}
LEAF_COUNTERS = {'batches', 'visits', 'materialized_outcomes', 'emitted_instructions'}
ARCHIVE_SHA256 = '27aefcba3fce289c7712905b85b14d4ab4a108a74cd7846bc9aaa1f115c23727'
ORACLES = {
    'oracle/fixture/rr-attract-undecided-gender.turn.json': 'gender-mixture-v1',
    'oracle/fixture/rr-cute-charm-undecided-gender.turn.json': 'gender-mixture-v1',
    'oracle/fixture/rr-rivalry-undecided-gender.turn.json': 'gender-mixture-v1',
    'oracle/fixture/ss-redirect-tie-hidden-order.turn.json': 'hidden-redirect-order-v1',
}
SCOPED = {
    'turn/fixture/bb-choicelock-struggle': 1,
    'turn/fixture/hyper-beam-recharge': 1,
    'turn/fixture/nn-pressure-locked-outrage': 1,
    'turn/fixture/red-card-drag-update': 2,
    'turn/fixture/u-truant-recharge': 1,
}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def validate_activation_probe(document):
    """Recompute every fixed control's verdict; claimed passed/equal bits alone never suffice."""
    require(isinstance(document, dict), 'Missing activation probe object')
    require(type(document.get('schema')) is int and document['schema'] == 1
            and document.get('suite') == ACTIVATION_SUITE, 'Wrong activation probe identity')
    require(document.get('config') == ACTIVATION_CONFIG
            and type(document['config'].get('depth')) is int
            and type(document['config'].get('threads')) is int
            and document['config'].get('factored') is False, 'Wrong activation workload')
    features = document.get('features', {})
    require(isinstance(features, dict) and set(features) == ACTIVATION_FEATURES
            and all(value is True for value in features.values()), 'Missing compiled activation feature')
    controls = document.get('controls')
    require(isinstance(controls, list) and len(controls) == len(ACTIVATION_IDS)
            and all(isinstance(row, dict) for row in controls)
            and [row.get('id') for row in controls] == ACTIVATION_IDS,
            'Missing, duplicate, reordered or unexpected activation control')
    for row in controls:
        sides = []
        for name, requested in [('on', True), ('off', False)]:
            side = row.get(name)
            require(isinstance(side, dict) and side.get('prepared_requested') is requested
                    and side.get('restored') is True and side.get('successful') is True,
                    'Activation toggle/success/restoration missing')
            signature = side.get('signature')
            require(isinstance(signature, str) and bool(signature.strip()), 'Missing result/work signature')
            counts = side.get('validation_counts')
            require(isinstance(counts, list) and len(counts) == 3
                    and all(type(value) is int and value > 0 for value in counts),
                    'Invalid three-validator evidence')
            leaf = side.get('leaf')
            require(isinstance(leaf, dict) and set(leaf) == LEAF_COUNTERS
                    and all(type(value) is int and value >= 0 for value in leaf.values())
                    and all(leaf[key] > 0 for key in ('batches', 'visits', 'materialized_outcomes')),
                    'Both leaf and nonleaf work must execute in each control')
            sides.append(side)
        on, off = sides
        require(on['signature'] == off['signature'] and on['leaf'] == off['leaf']
                and row.get('equal') is True, 'Activation output/work differs')
        require(all(a < b for a, b in zip(on['validation_counts'], off['validation_counts']))
                and row.get('passed') is True, 'Every validator must execute strictly fewer times')
    require(document.get('passed') is True, 'Activation probe declared failure')


def validate_precedence(prepared):
    """The frozen deep controls prove P9 precedence, not P8c activation."""
    require(isinstance(prepared, dict) and prepared.get('passed') is True
            and prepared.get('control_selection') == 'frozen-plan-first-eight-deep-single-thread'
            and prepared.get('purpose') == 'leaf-precedence', 'Missing frozen precedence controls')
    ids, controls = prepared.get('expected_control_ids'), prepared.get('controls')
    require(isinstance(ids, list) and len(ids) == 8 and all(isinstance(key, str) for key in ids)
            and len(set(ids)) == 8
            and isinstance(controls, list) and len(controls) == 8
            and all(isinstance(row, dict) for row in controls)
            and [row.get('id') for row in controls] == ids, 'Frozen precedence controls changed')
    for row in controls:
        require(row.get('equal') is True and row.get('toggle_confirmed') is True
                and row.get('leaf_active_both') is True, 'Precedence output/toggle/P9 evidence missing')
        on, off = row.get('on_parent_checks'), row.get('off_parent_checks')
        require(type(on) is int and type(off) is int and on > 0 and on == off,
                'Deep controls must retain positive equal validation counts')
        for name, requested, count in [('on', True, on), ('off', False, off)]:
            meta = row.get(name + '_metadata', {})
            require(isinstance(meta, dict) and meta.get('prepared_requested') is requested
                    and meta.get('requested_threads') == 1
                    and type(meta.get('requested_threads')) is int
                    and meta.get('factored') is False
                    and all(meta.get(flag) is True for flag in
                            ('prepared_compiled', 'prepared_observer_compiled', 'leaf_observer_compiled'))
                    and meta.get('prepared', {}).get('parent_checks') == count,
                    'Precedence feature/toggle evidence disagrees with verdict')
            leaf = meta.get('leaf', {})
            require(isinstance(leaf, dict) and all(type(leaf.get(key)) is int and leaf[key] > 0
                    for key in ('batches', 'visits')), 'P9 must execute in both precedence controls')


def validate_nonleaf_receipt(receipt):
    require(isinstance(receipt, dict) and receipt.get('passed') is True
            and receipt.get('returncode') == 0 and type(receipt.get('returncode')) is int
            and receipt.get('timeout') is False, 'Separate activation process did not complete')
    require(isinstance(receipt.get('binary_sha256'), str)
            and re.fullmatch('[0-9a-f]{64}', receipt['binary_sha256']) is not None
            and isinstance(receipt.get('stdout_sha256'), str)
            and re.fullmatch('[0-9a-f]{64}', receipt['stdout_sha256']) is not None,
            'Missing activation binary/output identity')
    validate_activation_probe(receipt.get('probe'))


def validate_variants(document):
    variants = document['variants']
    require(len(variants) == 2, 'Exactly two combined variants are required')
    off, on = variants
    require(off['id'] == 'combined-off' and on['id'] == 'combined-on', 'Unexpected variant identities/order')
    require(re.fullmatch('[0-9a-f]{40}', off['sha']) and off['sha'] != '0' * 40, 'Unresolved source SHA')
    require(off['sha'] == on['sha'], 'Combined variants must use the same source')
    require(off['features'] == [] and off['compare_to'] is None, 'Combined baseline must disable every option')
    require(len(on['features']) == len(FEATURES) and set(on['features']) == FEATURES,
            'Combined accuracy must enable all four options and both search observers')
    require(on['compare_to'] == 'combined-off', 'Wrong comparison baseline')


def validate_plan(plan):
    jobs = plan['jobs']
    require(len(jobs) == TOTAL and Counter(j['kind'] for j in jobs) == COUNTS,
            'Frozen evaluation denominator changed')
    require(len({j['id'] for j in jobs}) == TOTAL, 'Duplicate case IDs')
    require(plan['archive_sha256'] == ARCHIVE_SHA256, 'Frozen corpus changed')
    require({j['id']: j['oracle_contract'] for j in jobs if j.get('oracle_contract')} == ORACLES,
            'Special oracle coverage changed')
    by_id = {j['id']: j for j in jobs}
    require(all(key in by_id and '--before' in by_id[key]['args'] for key in SCOPED),
            'Legality scope contracts missing')
    controls = [j for j in jobs if j['kind'] == 'search' and j['args'][1] == 'deep' and j['args'][5] == '1'][:8]
    require(len(controls) == 8, 'Missing predetermined depth-two prepared controls')


def validate_summary(summary):
    require(summary.get('complete') is True and summary.get('all_requested_successful') is True
            and summary.get('activation_passed') is True and summary.get('validation_errors') == [],
            'Agreement controller did not pass')
    require(summary.get('expected_cases') == TOTAL, 'Wrong summary denominator')
    require(set(summary['comparisons']) == {'combined-on'}, 'Wrong comparison set')
    comparison = summary['comparisons']['combined-on']
    require(comparison['baseline'] == 'combined-off' and comparison['equal_success'] == TOTAL
            and all(comparison[name] == 0 for name in ('equal_error', 'different', 'uncompared')),
            'Some requested cases did not agree successfully')
    require({kind: row['equal_success'] for kind, row in comparison['by_kind'].items()} == COUNTS,
            'Per-kind denominator changed')
    require(set(summary['oracle']) == {'combined-off', 'combined-on'}, 'Missing variant oracle results')
    for variant in ('combined-off', 'combined-on'):
        require(summary['oracle'][variant] == {'match': COUNTS['oracle']}, 'Oracle mismatch or omission')
        contracts = summary['oracle_contracts'][variant]
        require(len(contracts) == len(ORACLES) and {r['id']: r['contract'] for r in contracts} == ORACLES,
                'Special oracle result missing')
        for row in contracts:
            require(row['status'] == 'match' and row['checks']['all_states_restored'] is True,
                    'Special oracle did not match with restoration')
            if row['contract'] == 'gender-mixture-v1':
                require(row['matching_positions'] == 16 and row['checks']['distinct_assignments'] == 16
                        and row['checks']['expected_assignments'] == 16, 'Incomplete gender enumeration')
            else:
                require(row['matching_positions'] == 2 and row['checks']['direct_oracle_histories'] == 2
                        and row['checks']['metamorphic_histories'] == 0, 'Missing independent redirect history')
        scoped = summary['scoped_turn_coverage'][variant]
        require(len(scoped) == len(SCOPED)
                and {r['id']: r['expected_choice_rejections'] for r in scoped} == SCOPED,
                'Expected additional legality rejections changed')
        require(all(r['status'] == 'ok' and r['success_scope'] == 'requested'
                    and r['scopes']['requested']['errors'] == 0
                    and r['scopes']['global']['errors'] == 0
                    and r['all_positions_status'] == 'error' for r in scoped),
                'Requested fixture success and additional rejected choices were conflated')
    activation = summary['activation']['combined-on']
    require(activation.get('leaf_required') is True and activation.get('prepared_required') is True
            and activation.get('leaf', {}).get('passed') is True
            and activation.get('prepared', {}).get('passed') is True,
            'Combined observer activations did not both pass')
    validate_precedence(activation.get('prepared'))
    validate_nonleaf_receipt(activation.get('prepared_nonleaf'))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--variants', type=Path)
    parser.add_argument('--plan', type=Path)
    parser.add_argument('--summary', type=Path)
    args = parser.parse_args()
    require(any((args.variants, args.plan, args.summary)), 'No contract input selected')
    for name, check in [('variants', validate_variants), ('plan', validate_plan), ('summary', validate_summary)]:
        path = getattr(args, name)
        if path:
            check(json.loads(path.read_text(encoding='utf-8-sig')))
            print(f'Combined {name} contract passed')


if __name__ == '__main__':
    main()
