"""In-memory oracle fixtures only; no Rust, Showdown, network, or timings."""
import copy
import gzip
import json
from pathlib import Path
import tarfile
import tempfile
import unittest

import pack_corpus as pack


def job(identifier, matchup, game, turn, before, eligible=True):
    return {'id': identifier, 'set': 'fixture', 'matchup': matchup, 'game': game,
            'turn': turn, 'stage': pack.stage(turn), 'before_sha256': before,
            'kind': 'Turn', 'policy': 'random', 'mode': 'full', 'before_features': {},
            'search_boundary_eligible': eligible}


class SelectionTests(unittest.TestCase):
    def test_semantic_dedup_game_matchup_coverage_and_boundary_filter(self):
        jobs = [job('a1', 'a', 'a-game-1', 2, 'one'),
                job('a1-duplicate', 'a', 'a-game-1', 2, 'one'),
                job('a2', 'a', 'a-game-2', 8, 'two'),
                job('a3', 'a', 'a-game-1', 20, 'three'),
                job('b1', 'b', 'b-game', 2, 'four'),
                job('replacement', 'c', 'c-game', 2, 'five', False)]
        selected, audit = pack.select_search(jobs, 4)
        self.assertEqual({x['id'] for x in selected}, {'a1', 'b1', 'a2', 'a3'})
        self.assertEqual(pack.select_search(list(reversed(jobs)), 4), (selected, audit))
        self.assertEqual(audit['eligible_entries'], 5)
        self.assertEqual(audit['unique_eligible_before_states'], 4)
        self.assertEqual(audit['selected']['games'], 3)
        self.assertEqual(audit['selected']['matchups'], 2)
        self.assertEqual(set(audit['selected']['stage']), {'early', 'middle', 'late'})
        with self.assertRaisesRegex(ValueError, 'unique eligible'):
            pack.select_search(jobs, 5)

    def test_canonical_before_hash_does_not_depend_on_object_order(self):
        self.assertEqual(pack.canonical({'a': 1, 'b': {'x': 2}}),
                         pack.canonical({'b': {'x': 2}, 'a': 1}))


class PackTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.corpus = self.root/'corpus'
        self.corpus.mkdir()
        self.index = {'schema': 1, 'oracle': {'commit': 'historical-only'}, 'positions': []}
        before = {'turn': 2, 'ended': False, 'field': {'weather': 'sunnyday'},
                  'sides': [{'request': 'move', 'pokemon': []}, {'request': 'move', 'pokemon': []}]}
        self.add_position('one', before)
        replacement = copy.deepcopy(before)
        replacement['sides'][0]['request'] = 'switch'
        self.add_position('replacement', replacement, kind='Replacement')
        self.index['positions'].append({'id': 'fixture/excluded', 'build': 'excluded',
                                       'excluded': 'stale pin, retain reason'})
        self.write_index()

    def write_index(self):
        (self.corpus/'corpus.json').write_bytes(pack.canonical(self.index))

    def gz(self, name, value):
        path = self.corpus/name
        path.parent.mkdir(parents=True, exist_ok=True)
        raw = pack.canonical(value)
        compressed = gzip.compress(raw, mtime=0)
        path.write_bytes(compressed)
        return len(raw), len(compressed)

    def add_position(self, name, before, kind='Turn'):
        scenario = f'positions/fixture/{name}.scenario.json.gz'
        sizes = self.gz(scenario, {'p1': {'team': []}, 'p2': {'team': []}, 'turn': {'p1': 'move 1'}})
        report = f'positions/fixture/{name}.full.report.json.gz'
        report_sizes = self.gz(report, {'mode': 'full', 'exact': True,
                     'showdownCommit': 'a'*40, 'before': before,
                     'outcomes': [{'p': 1, 'state': before}]})
        self.index['positions'].append({'id': 'fixture/'+name, 'build': 'ok',
            'set': 'fixture', 'game': 'game-'+name, 'matchup': 'pair-'+name,
            'policy': 'random', 'kind': kind, 'turn': before['turn'],
            'scenario': scenario, 'scenario_bytes': sizes[0], 'scenario_gz_bytes': sizes[1],
            'reports': [{'file': report, 'mode': 'full', 'label': 'full', 'roll': None,
                         'showdown': 'a'*40, 'outcomes': 1,
                         'bytes': report_sizes[0], 'gz_bytes': report_sizes[1]}]})

    def run_pack(self, name='data'):
        return pack.make_pack(self.corpus, self.root/name, 1, 2, 2)

    def test_original_gzip_and_provenance_preserved_deterministically(self):
        original = {p.relative_to(self.corpus).as_posix(): p.read_bytes()
                    for p in self.corpus.rglob('*') if p.is_file()}
        first = self.run_pack()
        second = self.run_pack('second')
        self.assertEqual(first['archive_sha256'], second['archive_sha256'])
        self.assertEqual(first['counts']['positions'], 2)
        self.assertEqual(first['counts']['reports'], 2)
        self.assertEqual(first['counts']['excluded'], 1)
        self.assertEqual(first['counts']['unique_raw_scenarios'], 1)
        self.assertEqual(len(first['search_jobs']), 1)
        self.assertEqual(first['search_jobs'][0]['kind'], 'Turn')
        self.assertTrue(first['archive_readback_verified'])
        with tarfile.open(self.root/'data'/pack.ARCHIVE, 'r:gz') as archive:
            self.assertEqual({p.name: archive.extractfile(p).read() for p in archive}, original)
        self.assertEqual({p.relative_to(self.corpus).as_posix(): p.read_bytes()
                         for p in self.corpus.rglob('*') if p.is_file()}, original)
        with self.assertRaisesRegex(ValueError, 'new directory'):
            self.run_pack()

    def test_stale_size_and_pending_entry_refused_before_output(self):
        self.index['positions'][0]['scenario_bytes'] += 1
        self.write_index()
        with self.assertRaisesRegex(ValueError, 'size differs'):
            self.run_pack()
        self.assertFalse((self.root/'data').exists())
        self.index['positions'][0]['build'] = 'pending'
        self.write_index()
        with self.assertRaisesRegex(ValueError, 'Pending or failed'):
            self.run_pack()

    def test_missing_extra_and_escaping_inputs_are_rejected(self):
        extra = self.corpus/'positions/unindexed.json.gz'
        extra.write_bytes(gzip.compress(b'{}'))
        with self.assertRaisesRegex(ValueError, 'inventory'):
            self.run_pack()
        extra.unlink()
        self.index['positions'][0]['scenario'] = '../outside.gz'
        self.write_index()
        with self.assertRaisesRegex(ValueError, 'Unsafe corpus path'):
            self.run_pack()

    def test_duplicate_ids_or_non_exact_oracle_cannot_pass(self):
        self.index['positions'][1]['id'] = self.index['positions'][0]['id']
        self.write_index()
        with self.assertRaisesRegex(ValueError, 'Duplicate corpus position'):
            self.run_pack()
        self.index['positions'][1]['id'] = 'fixture/replacement'
        entry = self.index['positions'][0]['reports'][0]
        raw = json.loads(gzip.decompress((self.corpus/entry['file']).read_bytes()))
        raw['exact'] = False
        entry['bytes'], entry['gz_bytes'] = self.gz(entry['file'], raw)
        self.write_index()
        with self.assertRaisesRegex(ValueError, 'provenance/exactness'):
            self.run_pack()

    def test_duplicate_json_keys_rejected(self):
        with self.assertRaisesRegex(ValueError, 'Duplicate JSON key'):
            pack.parse(b'{"mode":"full","mode":"extremes"}')

    def test_reduced_roll_exact_false_is_retained_as_reduced_not_rejected(self):
        entry = self.index['positions'][0]['reports'][0]
        raw = json.loads(gzip.decompress((self.corpus/entry['file']).read_bytes()))
        raw.update(mode='extremes', exact=False)
        entry.update(mode='extremes', label='extremes')
        entry['bytes'], entry['gz_bytes'] = self.gz(entry['file'], raw)
        self.write_index()
        result = self.run_pack()
        chosen = next(j for j in result['oracle_jobs'] if j['position_id']=='fixture/one')
        self.assertEqual(chosen['mode'], 'extremes')
        self.assertFalse(chosen['oracle_full_exact'])


if __name__ == '__main__':
    unittest.main()
