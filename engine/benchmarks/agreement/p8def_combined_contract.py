"""Fresh old4 versus old4+P8d/e/f agreement, with the existing P8e exception.

Only turn outcome instruction text may differ. The original streamed output,
its SHA/byte count, and every other state/probability/error/order field survive.
Search and oracle retain their existing exact comparison contracts.
"""
import argparse
import copy
import json
from pathlib import Path
import re

import combined_contract as previous
import p8def_contract as independent

SOURCE_SHA = '048e4748e8854dac5eb511a358cc2f8c38c9dc29'
CANDIDATE = 'p8def_combined'
BASE_FEATURES = previous.FEATURES
FEATURES = BASE_FEATURES | set(independent.FEATURES.values())
STATE_CONTRACT = independent.STATE_CONTRACT
require = previous.require
validate_plan = previous.validate_plan
turn_state_digest = independent.turn_state_digest


def validate_candidate(variant):
    require(variant.get('id') == CANDIDATE and variant.get('compare_to') == 'base',
            'Wrong combined candidate/baseline')
    require(variant.get('sha') == SOURCE_SHA, 'Combined source must be frozen 048e4748')
    features = variant.get('features')
    require(isinstance(features, list) and len(features) == len(FEATURES)
            and set(features) == FEATURES, 'Combined candidate requires exactly old4 plus P8d/e/f')
    require(variant.get('comparison_contract') == STATE_CONTRACT,
            'Combined equality contract must retain only the P8e instruction exception')


def validate_variants(document):
    variants = document.get('variants')
    require(isinstance(variants, list) and len(variants) == 2
            and all(isinstance(v, dict) for v in variants)
            and [v.get('id') for v in variants] == ['base', CANDIDATE],
            'Exactly two ordered combined variants required')
    base, candidate = variants
    require(base.get('sha') == SOURCE_SHA, 'Baseline source must be frozen 048e4748')
    features = base.get('features')
    require(isinstance(features, list) and len(features) == len(BASE_FEATURES)
            and set(features) == BASE_FEATURES, 'Baseline requires exactly old4 and its observers')
    require(base.get('compare_to') is None
            and base.get('comparison_contract', 'exact-v1') == 'exact-v1', 'Wrong baseline contract')
    validate_candidate(candidate)


def comparison_equal(variant, kind, candidate, original):
    validate_candidate(variant)
    for record in (candidate, original):
        require(isinstance(record.get('sha256'), str)
                and re.fullmatch('[0-9a-f]{64}', record['sha256']), 'Missing raw stream SHA')
        require(type(record.get('stdout_bytes')) is int and record['stdout_bytes'] > 0,
                'Missing raw stream byte count')
    if kind == 'turn':
        hashes = [record.get('turn_state_sha256') for record in (candidate, original)]
        require(all(isinstance(value, str) and re.fullmatch('[0-9a-f]{64}', value)
                    for value in hashes), 'Missing semantic turn digest')
        return hashes[0] == hashes[1]
    return candidate['sha256'] == original['sha256']


def validate_summary(summary):
    require(set(summary.get('comparisons', {})) == {CANDIDATE},
            'One simultaneous P8d/e/f comparison required')
    for name in ('oracle', 'oracle_contracts', 'scoped_turn_coverage', 'activation'):
        require(set(summary.get(name, {})) == {'base', CANDIDATE},
                'Missing combined variant evidence: ' + name)
    require(all(summary['activation'][name].get('passed') is True for name in ('base', CANDIDATE)),
            'Combined activation result failed')
    comparison = summary['comparisons'][CANDIDATE]
    require(comparison.get('baseline') == 'base'
            and comparison.get('comparison_contract') == STATE_CONTRACT,
            'Wrong combined baseline/equality contract')
    ids = comparison.get('raw_turn_different_ids')
    require(isinstance(ids, list) and all(isinstance(key, str) and key.startswith('turn/') for key in ids)
            and ids == sorted(set(ids)) and len(ids) <= previous.COUNTS['turn'],
            'Missing or malformed raw representation ID ledger')
    rows = comparison.get('raw_turn_differences')
    require(isinstance(rows, list) and all(isinstance(row, dict) for row in rows)
            and [row.get('id') for row in rows] == ids, 'Missing raw representation evidence')
    for row in rows:
        for side in ('baseline', 'candidate'):
            value = row.get(side + '_sha256')
            require(isinstance(value, str) and re.fullmatch('[0-9a-f]{64}', value),
                    'Invalid raw representation SHA')
            value = row.get(side + '_stdout_bytes')
            require(type(value) is int and value > 0, 'Invalid raw representation byte count')
        require(row['baseline_sha256'] != row['candidate_sha256'], 'Spurious raw difference')

    # Reuse the frozen 6,184-case oracle/legality/activation contract without
    # relaxing a denominator or substituting independent experiment results.
    view = copy.deepcopy(summary)
    view['comparisons'] = {'combined-on': copy.deepcopy(comparison)}
    view['comparisons']['combined-on']['baseline'] = 'combined-off'
    for name in ('oracle', 'oracle_contracts', 'scoped_turn_coverage', 'activation'):
        view[name] = {'combined-off': summary[name]['base'],
                      'combined-on': summary[name][CANDIDATE]}
    previous.validate_summary(view)
    # Both sides have prepared+leaf enabled. Require the same complete activation
    # proof for the baseline, rather than trusting only its top-level passed bit.
    view['activation']['combined-on'] = summary['activation']['base']
    previous.validate_summary(view)


def main():
    parser = argparse.ArgumentParser()
    for name in ('variants', 'plan', 'summary'):
        parser.add_argument('--' + name, type=Path)
    args = parser.parse_args()
    require(any(vars(args).values()), 'No contract input')
    for name in ('variants', 'plan', 'summary'):
        path = getattr(args, name)
        if path:
            globals()['validate_' + name](json.loads(path.read_text(encoding='utf-8-sig')))
            print('P8def combined ' + name + ' contract passed')


if __name__ == '__main__':
    main()
