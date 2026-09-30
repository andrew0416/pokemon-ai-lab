"""Fresh old4+D versus old4+D+P11; no instruction-representation exception."""
import argparse
import copy
import json
from pathlib import Path
import re

import combined_contract as previous

SOURCE_SHA = '1d0c679fa35d43b530a8ee4ab8d5d8dc3f04af64'
CANDIDATE = 'p11'
BASE_FEATURES = previous.FEATURES | {'lab-engine/experiment-replay-action-keys'}
FEATURES = BASE_FEATURES | {'lab-engine/experiment-inline-runstart'}
require = previous.require
validate_plan = previous.validate_plan


def validate_candidate(variant):
    require(variant.get('id') == CANDIDATE and variant.get('compare_to') == 'base',
            'Wrong P11 candidate/baseline')
    require(variant.get('sha') == SOURCE_SHA, 'P11 source must be frozen 1d0c679f')
    features = variant.get('features')
    require(isinstance(features, list) and len(features) == len(FEATURES)
            and set(features) == FEATURES, 'P11 candidate must enable exactly old4+D+P11')
    require(variant.get('comparison_contract') == 'exact-v1', 'P11 requires exact raw output')


def validate_variants(document):
    variants = document.get('variants')
    require(isinstance(variants, list) and len(variants) == 2
            and all(isinstance(v, dict) for v in variants)
            and [v.get('id') for v in variants] == ['base', CANDIDATE],
            'Exactly two ordered P11 variants required')
    base, candidate = variants
    require(base.get('sha') == SOURCE_SHA, 'P11 baseline source must match candidate')
    features = base.get('features')
    require(isinstance(features, list) and len(features) == len(BASE_FEATURES)
            and set(features) == BASE_FEATURES, 'P11 baseline requires exactly old4+D and search observers')
    require(base.get('compare_to') is None
            and base.get('comparison_contract', 'exact-v1') == 'exact-v1', 'Wrong baseline contract')
    validate_candidate(candidate)


def comparison_equal(variant, kind, candidate, original):
    validate_candidate(variant)
    require(kind in ('turn', 'search'), 'Oracle must use the existing semantic oracle comparator')
    for record in (candidate, original):
        require(isinstance(record.get('sha256'), str)
                and re.fullmatch('[0-9a-f]{64}', record['sha256']), 'Missing raw stream SHA')
        require(type(record.get('stdout_bytes')) is int and record['stdout_bytes'] > 0,
                'Missing raw stream byte count')
    return (candidate['sha256'], candidate['stdout_bytes']) == (original['sha256'], original['stdout_bytes'])


def validate_summary(summary):
    require(set(summary.get('comparisons', {})) == {CANDIDATE}, 'Exactly one P11 comparison required')
    for name in ('oracle', 'oracle_contracts', 'scoped_turn_coverage', 'activation'):
        require(set(summary.get(name, {})) == {'base', CANDIDATE}, 'Missing P11 variant evidence: ' + name)
    comparison = summary['comparisons'][CANDIDATE]
    require(comparison.get('baseline') == 'base' and comparison.get('comparison_contract') == 'exact-v1',
            'Wrong P11 baseline/equality contract')
    require(comparison.get('raw_turn_different_ids') == [] and comparison.get('raw_turn_differences') == [],
            'P11 cannot hide a raw instruction difference')
    view = copy.deepcopy(summary)
    view['comparisons'] = {'combined-on': copy.deepcopy(comparison)}
    view['comparisons']['combined-on']['baseline'] = 'combined-off'
    for name in ('oracle', 'oracle_contracts', 'scoped_turn_coverage', 'activation'):
        view[name] = {'combined-off': summary[name]['base'], 'combined-on': summary[name][CANDIDATE]}
    previous.validate_summary(view)
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
            print('P11 ' + name + ' contract passed')


if __name__ == '__main__':
    main()
