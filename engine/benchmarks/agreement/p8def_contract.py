"""Independent P8d/e/f contracts, preserving the frozen oracle and state checks."""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import re
import combined_contract as previous

FEATURES = {'p8d': 'lab-engine/experiment-replay-action-keys',
            'p8e': 'lab-engine/experiment-slot-diff',
            'p8f': 'lab-engine/experiment-stats-off-cost'}
STATE_CONTRACT = 'state-equivalent-slot-diff-v1'
require = previous.require
validate_plan = previous.validate_plan

def validate_variants(document):
    variants = document['variants']
    require([v['id'] for v in variants] == ['base', 'p8d', 'p8e', 'p8f'], 'Four ordered variants required')
    base = variants[0]
    require(re.fullmatch('[0-9a-f]{40}', base['sha']) and base['sha'] != '0' * 40, 'Unresolved source')
    for v in variants:
        require(v['sha'] == base['sha'], 'All variants must share source')
        want = previous.FEATURES | ({FEATURES[v['id']]} if v['id'] != 'base' else set())
        require(len(v['features']) == len(want) and set(v['features']) == want, 'Unexpected feature combination')
        require(v['compare_to'] == (None if v['id'] == 'base' else 'base'), 'Wrong baseline')
        require(v.get('comparison_contract', 'exact-v1') == (STATE_CONTRACT if v['id'] == 'p8e' else 'exact-v1'), 'Wrong equality contract')

def turn_state_digest(rows):
    """Only emitted outcome instructions are representation-dependent; preserve all other data.

    run_case calls validate_probe first, and Rust performs full apply/reverse and
    incremental hash assertions before successful process completion.
    """
    digest = hashlib.sha256()
    for source in rows:
        require(isinstance(source, dict) and isinstance(source.get('kind'), str), 'Malformed turn row')
        row = dict(source)
        if row['kind'] == 'outcome':
            require(isinstance(row.get('instructions'), str), 'Missing instruction evidence')
            require(row.get('input_restored') is True and row.get('incremental_hash_checked') is True,
                    'Missing rollback/hash evidence')
            require(all(k in row for k in ('probability_bits', 'position', 'outcome', 'suspension', 'party_order', 'hidden')), 'Missing outcome contract')
            require(isinstance(row.get('state'), dict) and
                    all(k in row['state'] for k in ('debug', 'key_hash', 'position_hash')), 'Missing full State/hash evidence')
            del row['instructions']
        digest.update(json.dumps(row, ensure_ascii=False, sort_keys=True, separators=(',', ':'), allow_nan=False).encode('utf8'))
        digest.update(b'\n')
    return digest.hexdigest()

def comparison_equal(variant, kind, candidate, original):
    contract = variant.get('comparison_contract', 'exact-v1')
    if contract == STATE_CONTRACT:
        require(variant['id'] == 'p8e' and variant['compare_to'] == 'base'
                and set(variant['features']) == previous.FEATURES | {FEATURES['p8e']}, 'Unauthorized representation exemption')
        if kind == 'turn':
            hashes = [r.get('turn_state_sha256') for r in (candidate, original)]
            require(all(isinstance(h, str) and re.fullmatch('[0-9a-f]{64}', h) for h in hashes), 'Missing semantic digest')
            return hashes[0] == hashes[1]
    else:
        require(contract == 'exact-v1', 'Unknown equality contract')
    return candidate['sha256'] == original['sha256']

def validate_summary(summary):
    require(set(summary['comparisons']) == set(FEATURES), 'Three independent comparisons required')
    for name in ('oracle', 'oracle_contracts', 'scoped_turn_coverage', 'activation'):
        require(set(summary[name]) == {'base', *FEATURES}, 'Missing variant evidence: ' + name)
    # Reuse every original denominator/oracle/legality/activation requirement.
    for candidate in FEATURES:
        view = copy.deepcopy(summary)
        view['comparisons'] = {'combined-on': copy.deepcopy(summary['comparisons'][candidate])}
        comparison = view['comparisons']['combined-on']
        require(comparison['baseline'] == 'base', 'Wrong independent baseline')
        require(comparison.get('comparison_contract') == (STATE_CONTRACT if candidate == 'p8e' else 'exact-v1'), 'Untracked equality contract')
        differences = comparison.get('raw_turn_different_ids')
        require(isinstance(differences, list) and len(differences) == len(set(differences))
                and all(isinstance(k, str) and k.startswith('turn/') for k in differences), 'Missing raw representation ledger')
        if candidate != 'p8e':
            require(not differences, 'Exact candidate has different turn output')
        comparison['baseline'] = 'combined-off'
        for name in ('oracle', 'oracle_contracts', 'scoped_turn_coverage', 'activation'):
            view[name] = {'combined-off': summary[name]['base'], 'combined-on': summary[name][candidate]}
        previous.validate_summary(view)
    base = summary['activation']['base']
    require(base.get('passed') is True and base.get('leaf', {}).get('passed') is True, 'Base activation missing')
    previous.validate_precedence(base.get('prepared'))
    previous.validate_nonleaf_receipt(base.get('prepared_nonleaf'))

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
            print('P8def ' + name + ' contract passed')

if __name__ == '__main__':
    main()
