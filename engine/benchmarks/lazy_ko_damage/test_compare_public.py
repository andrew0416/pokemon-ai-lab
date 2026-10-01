import copy
import json
from pathlib import Path
import tempfile
import unittest
from compare_public import CASES, compare_records


class Records(unittest.TestCase):
    def make(self):
        return [dict(schema=1, case=name, rolls='Full' if name == 'all-ko' else 'Extremes',
                     input='State { fixture }', input_hash=123, components=2,
                     distribution={'State { hidden: 1 }\nNone': 0.5, 'State { hidden: 2 }\nNone': 0.5},
                     samples={'State { hidden: 1 }\nNone': 1.0}) for name in CASES]

    def compare(self, left, right):
        with tempfile.TemporaryDirectory() as folder:
            paths = [Path(folder)/'off.jsonl', Path(folder)/'on.jsonl']
            for p, rows in zip(paths, (left, right)):
                p.write_text(''.join(json.dumps(row)+'\n' for row in rows), encoding='utf-8')
            return compare_records(*paths)

    def test_only_roundoff_and_component_layout_may_change(self):
        a = self.make(); b = copy.deepcopy(a)
        b[0]['components'] = 1
        b[0]['distribution']['State { hidden: 1 }\nNone'] += 1e-14
        b[0]['distribution']['State { hidden: 2 }\nNone'] -= 1e-14
        self.assertTrue(self.compare(a, b)['passed'])

    def test_hidden_state_and_pending_change_rejected(self):
        for key in ('State { hidden: 3 }\nNone', 'State { hidden: 1 }\nSome(pending)'):
            a = self.make(); b = copy.deepcopy(a)
            b[0]['distribution'][key] = b[0]['distribution'].pop('State { hidden: 1 }\nNone')
            with self.assertRaises(ValueError): self.compare(a, b)

    def test_mass_and_probability_error_rejected(self):
        for delta in (0.1, -0.1):
            a = self.make(); b = copy.deepcopy(a)
            b[0]['distribution']['State { hidden: 1 }\nNone'] += delta
            b[0]['distribution']['State { hidden: 2 }\nNone'] -= delta
            with self.assertRaises(ValueError): self.compare(a, b)

    def test_missing_case_and_sample_change_rejected(self):
        a = self.make()
        with self.assertRaises(ValueError): self.compare(a, a[:-1])
        b = copy.deepcopy(a)
        b[0]['samples'] = {'State { hidden: 2 }\nNone': 1.0}
        with self.assertRaises(ValueError): self.compare(a, b)


if __name__ == '__main__': unittest.main()
