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
