"""Fresh R1 versus R1+P15; no instruction-representation exception."""
import argparse
import copy
import json
from pathlib import Path
import re

import combined_contract as previous

SOURCE_SHA = '71d5a84a97f6a3ce05430e1108538aed43fdc7f1'
CANDIDATE = 'p15'
BASE_FEATURES = previous.FEATURES | {'lab-engine/experiment-replay-action-keys', 'lab-search/experiment-borrowed-child-keys'}
FEATURES = BASE_FEATURES | {'lab-search/experiment-prepared-leaf'}
require = previous.require
PRECEDENCE_CONTRACT = 'prepared-leaf-precedence-v1'
VALIDATORS = ('parent_checks', 'side_checks', 'support_checks')
CONTROL_IDS = [
    'search/deep/v2/dbl-kickoff-beedrillvgc-vs-kickoff-conkledonk.b.random.g000.s10',
    'search/deep/ff/gardevoir-vs-psy-cona.random.g002.s11',
    'search/deep/v2/mech-tomoe-garchomp-golisopod-vs-tororo-goodra-grassy.b.random.g000.s13',
    'search/deep/v2/dbl-kickoff-aveornot-vs-kickoff-balmung.b.random.g000.s04',
    'search/deep/v2/dbl-psy-lello-vs-psy-nihat.b.random.g001.s06',
    'search/deep/v2/mech-sarami-raichu-experiment-vs-tama-screens-revival.b.random.g000.s01',
    'search/deep/v2/dbl-kickoff-aveornot-vs-kickoff-balmung.a.nash.g000.s08',
    'search/deep/v2/mech-masagon-lucario-salamence-vs-mutsu-baxcalibur.a.random.g001.s02',
]


def validate_plan(plan):
    previous.validate_plan(plan)
    require([j['id'] for j in plan['jobs'] if j['kind']=='search'
             and j['args'][1]=='deep' and j['args'][5]=='1'][:8]==CONTROL_IDS,
            'P15 frozen first eight deep case identities/order changed')


def validate_prepared_leaf_precedence(prepared):
    """P15-only contract: same frozen work, newly reduced validation work.

    The immutable combined contract continues to check R1 and all older modes.
    Validation diagnostics are excluded from the exact semantic stdout stream.
    """
    require(isinstance(prepared, dict) and prepared.get('passed') is True
            and prepared.get('control_selection') == 'frozen-plan-first-eight-deep-single-thread'
            and prepared.get('purpose') == 'prepared-leaf-sharing'
            and prepared.get('contract') == PRECEDENCE_CONTRACT, 'Missing versioned P15 controls')
    ids, controls = prepared.get('expected_control_ids'), prepared.get('controls')
    require(isinstance(ids,list) and ids==CONTROL_IDS and len(set(ids))==8
            and all(isinstance(key,str) for key in ids) and isinstance(controls,list) and len(controls)==8
            and all(isinstance(row,dict) for row in controls) and [row.get('id') for row in controls]==ids,
            'P15 missing, duplicate, reordered or unexpected frozen control')
    reduced = False
    for row in controls:
        require(row.get('equal') is True and row.get('toggle_confirmed') is True
                and row.get('leaf_active_both') is True, 'P15 output, toggle or P9 evidence missing')
        digests = [row.get(side+'_stdout_sha256') for side in ('on','off')]
        lengths = [row.get(side+'_stdout_bytes') for side in ('on','off')]
        require(all(isinstance(d,str) and re.fullmatch('[0-9a-f]{64}',d) for d in digests)
                and digests[0]==digests[1] and all(type(n) is int and n>0 for n in lengths)
                and lengths[0]==lengths[1], 'P15 exact output and integer work stream differ')
        sides = []
        for name, requested in (('on',True),('off',False)):
            meta = row.get(name+'_metadata',{})
            require(isinstance(meta,dict) and meta.get('prepared_requested') is requested
                    and type(meta.get('requested_threads')) is int and meta['requested_threads']==1
                    and meta.get('factored') is False and meta.get('phase')=='search-complete'
                    and all(meta.get(flag) is True for flag in
                            ('prepared_compiled','prepared_observer_compiled','leaf_observer_compiled')),
                    'P15 fake/missing compiled observer or toggle evidence')
            counts = meta.get('prepared',{})
            require(isinstance(counts,dict) and set(counts)==set(VALIDATORS)
                    and all(type(counts.get(k)) is int and counts[k]>0 for k in VALIDATORS)
                    and row.get(name+'_parent_checks')==counts['parent_checks'], 'P15 validator evidence malformed')
            leaf=meta.get('leaf',{})
            require(isinstance(leaf,dict) and set(leaf)==previous.LEAF_COUNTERS
                    and all(type(v) is int and v>=0 for v in leaf.values())
                    and leaf['batches']>0 and leaf['visits']>0, 'P15 P9 work evidence missing')
            sides.append(meta)
        on,off=sides
        require(on['leaf']==off['leaf'], 'P15 leaf work changed while output stayed equal')
        require(all(on['prepared'][k]<=off['prepared'][k] for k in VALIDATORS),
                'P15 increased validation in a frozen control')
        reduced |= all(on['prepared'][k]<off['prepared'][k] for k in VALIDATORS)
    require(reduced, 'P15 zero shared validation activation across the frozen controls')


def validate_candidate_activation(activation):
    require(isinstance(activation,dict) and activation.get('passed') is True
            and activation.get('leaf_required') is True and activation.get('prepared_required') is True,
            'P15 activation not required/passed')
    leaf=activation.get('leaf',{})
    require(leaf.get('passed') is True and leaf.get('observer_compiled') is True
            and all(type(leaf.get(k)) is int and leaf[k]>0 for k in ('batches','visits','visited_cases')),
            'P15 actual P9 activation missing')
    validate_prepared_leaf_precedence(activation.get('prepared'))
    previous.validate_nonleaf_receipt(activation.get('prepared_nonleaf'))


def validate_candidate(variant):
    require(variant.get('id') == CANDIDATE and variant.get('compare_to') == 'base',
            'Wrong P15 candidate/baseline')
    require(variant.get('sha') == SOURCE_SHA, 'P15 source must match the frozen implementation')
    features = variant.get('features')
    require(isinstance(features, list) and len(features) == len(FEATURES)
            and set(features) == FEATURES, 'P15 candidate must enable exactly R1+P15')
    require(variant.get('comparison_contract') == 'exact-v1', 'P15 requires exact raw output')


def validate_variants(document):
    variants = document.get('variants')
    require(isinstance(variants, list) and len(variants) == 2
            and all(isinstance(v, dict) for v in variants)
            and [v.get('id') for v in variants] == ['base', CANDIDATE],
            'Exactly two ordered P15 variants required')
    base, candidate = variants
    require(base.get('sha') == SOURCE_SHA, 'P15 baseline source must match candidate')
    features = base.get('features')
    require(isinstance(features, list) and len(features) == len(BASE_FEATURES)
            and set(features) == BASE_FEATURES, 'P15 baseline requires exactly R1 and search observers')
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
    require(set(summary.get('comparisons', {})) == {CANDIDATE}, 'Exactly one P15 comparison required')
    for name in ('oracle', 'oracle_contracts', 'scoped_turn_coverage', 'activation'):
        require(set(summary.get(name, {})) == {'base', CANDIDATE}, 'Missing P15 variant evidence: ' + name)
    comparison = summary['comparisons'][CANDIDATE]
    require(summary['activation']['base'].get('prepared',{}).get('expected_control_ids')==CONTROL_IDS,
            'P15 R1 baseline controls differ from the frozen plan')
    require(comparison.get('baseline') == 'base' and comparison.get('comparison_contract') == 'exact-v1',
            'Wrong P15 baseline/equality contract')
    require(comparison.get('raw_turn_different_ids') == [] and comparison.get('raw_turn_differences') == [],
            'P15 cannot hide a raw instruction difference')
    view = copy.deepcopy(summary)
    view['comparisons'] = {'combined-on': copy.deepcopy(comparison)}
    view['comparisons']['combined-on']['baseline'] = 'combined-off'
    for name in ('oracle', 'oracle_contracts', 'scoped_turn_coverage', 'activation'):
        view[name] = {'combined-off': summary[name]['base'], 'combined-on': summary[name][CANDIDATE]}
    # Reuse the unchanged semantic/oracle/coverage validator with R1's original
    # precedence contract. Validate the candidate's distinct activation contract
    # explicitly; never rewrite its observed counts to resemble the old contract.
    validate_candidate_activation(summary['activation'][CANDIDATE])
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
            print('P15 ' + name + ' contract passed')


if __name__ == '__main__':
    main()
