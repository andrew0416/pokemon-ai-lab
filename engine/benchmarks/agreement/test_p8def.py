import copy
import unittest
import p8def_contract as c
import test_combined

def variants():
    rows = [{'id': 'base', 'sha': 'a' * 40, 'features': sorted(c.previous.FEATURES), 'compare_to': None}]
    for key, flag in c.FEATURES.items():
        rows.append({'id': key, 'sha': 'a' * 40, 'features': sorted(c.previous.FEATURES | {flag}),
                     'compare_to': 'base', 'comparison_contract': c.STATE_CONTRACT if key == 'p8e' else 'exact-v1'})
    return {'variants': rows}

def outcome():
    return {'kind': 'outcome', 'instructions': '[Switch]', 'position': 0, 'outcome': 0,
            'probability_bits': 123, 'suspension': 'None', 'party_order': '[0,1]',
            'hidden': {'known': True}, 'input_restored': True, 'incremental_hash_checked': True,
            'state': {'debug': 'full-state', 'key_hash': 100, 'position_hash': 200}}

class IndependentContractTests(unittest.TestCase):
    def test_four_same_source_and_one_extra_flag(self):
        c.validate_variants(variants())
        for mutate in [lambda v: v['variants'][0]['features'].clear(),
                       lambda v: v['variants'][1]['features'].append(c.FEATURES['p8f']),
                       lambda v: v['variants'][2].update(sha='b' * 40),
                       lambda v: v['variants'][3].update(compare_to='p8d'),
                       lambda v: v['variants'][1].update(comparison_contract=c.STATE_CONTRACT)]:
            v = variants(); mutate(v)
            with self.assertRaises(ValueError): c.validate_variants(v)

    def test_only_p8e_instruction_representation_can_differ(self):
        a, b = outcome(), outcome(); b['instructions'] = '[SetLastMove]'
        left = {'sha256': 'a', 'turn_state_sha256': c.turn_state_digest([a])}
        right = {'sha256': 'b', 'turn_state_sha256': c.turn_state_digest([b])}
        rows = variants()['variants']
        self.assertTrue(c.comparison_equal(rows[2], 'turn', left, right))
        for v in [rows[1], rows[3]]:
            self.assertFalse(c.comparison_equal(v, 'turn', left, right))
        self.assertFalse(c.comparison_equal(rows[2], 'search', left, right))
        with self.assertRaises(ValueError):
            c.comparison_equal(rows[2], 'turn', left, {'sha256': 'b'})
        with self.assertRaises(ValueError):
            c.comparison_equal({**rows[1], 'comparison_contract': c.STATE_CONTRACT}, 'turn', left, right)

    def test_all_state_probability_order_and_hidden_fields_are_retained(self):
        base = outcome(); original = c.turn_state_digest([base])
        for key, value in [('probability_bits', 124), ('position', 1), ('outcome', 1),
                           ('suspension', 'pending'), ('party_order', '[1,0]'), ('hidden', {'known': False}),
                           ('state', {**base['state'], 'debug': 'changed'}),
                           ('state', {**base['state'], 'key_hash': 101}),
                           ('state', {**base['state'], 'position_hash': 201})]:
            row = copy.deepcopy(base); row[key] = value
            self.assertNotEqual(c.turn_state_digest([row]), original, key)
        self.assertNotEqual(c.turn_state_digest([base, {'kind': 'complete', 'errors': 0}]),
                            c.turn_state_digest([{'kind': 'complete', 'errors': 0}, base]))

    def test_missing_rollback_or_full_state_rejected(self):
        for key in ('instructions', 'probability_bits', 'state', 'input_restored', 'incremental_hash_checked'):
            row = outcome(); del row[key]
            with self.assertRaises(ValueError): c.turn_state_digest([row])
        for key in ('input_restored', 'incremental_hash_checked'):
            row = outcome(); row[key] = False
            with self.assertRaises(ValueError): c.turn_state_digest([row])

    def test_instructions_outside_outcomes_are_not_exempt(self):
        self.assertNotEqual(c.turn_state_digest([{'kind': 'other', 'instructions': 'a'}]),
                            c.turn_state_digest([{'kind': 'other', 'instructions': 'b'}]))

    def test_all_four_variants_keep_full_denominator_oracle_and_activation_contracts(self):
        fixture = test_combined.CombinedContractTests(); fixture.setUp()
        original = fixture.summary()
        summary = copy.deepcopy(original)
        for name in ('oracle', 'oracle_contracts', 'scoped_turn_coverage', 'activation'):
            summary[name] = {key: copy.deepcopy(original[name]['combined-on']) for key in ('base', *c.FEATURES)}
        summary['comparisons'] = {}
        for candidate in c.FEATURES:
            row = copy.deepcopy(original['comparisons']['combined-on'])
            row.update(baseline='base', comparison_contract=c.STATE_CONTRACT if candidate == 'p8e' else 'exact-v1',
                       raw_turn_different_ids=['turn/fixture/sample'] if candidate == 'p8e' else [])
            summary['comparisons'][candidate] = row
        c.validate_summary(summary)
        for mutate in [lambda s: s['comparisons'].pop('p8f'),
                       lambda s: s['comparisons']['p8e'].update(equal_error=1),
                       lambda s: s['comparisons']['p8d'].update(raw_turn_different_ids=['turn/fixture/x']),
                       lambda s: s['oracle']['base'].update(match=3055),
                       lambda s: s['activation']['base'].update(passed=False),
                       lambda s: s['activation']['p8f'].pop('prepared_nonleaf'),
                       lambda s: s['scoped_turn_coverage']['p8e'][0].update(expected_choice_rejections=0)]:
            bad = copy.deepcopy(summary); mutate(bad)
            with self.assertRaises(ValueError): c.validate_summary(bad)

if __name__ == '__main__':
    unittest.main()
