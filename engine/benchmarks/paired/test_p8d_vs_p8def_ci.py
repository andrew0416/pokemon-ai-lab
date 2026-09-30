"""Direct common5 versus common7 measurement contracts; no Rust process runs.

Reuse the reviewed combined gate fault fixtures with this mode selected. This
checks real controller writers/validators, including missing runtime features,
unknown P11, observer contamination, raw representation differences and cache hits.
"""
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import ci
import compact_probe
import test_p8def_ci as independent
import test_p8def_combined_ci as combined

MODE = 'p8d-vs-p8def'


class DirectMode:
    def setUp(self):
        super().setUp()
        self.mode_patch = patch.object(combined, 'MODE', MODE)
        self.mode_patch.start()
        self.addCleanup(self.mode_patch.stop)


class DirectRoutingTests(DirectMode, combined.CombinedRoutingTests):
    def test_exact_common_four_vs_all_seven_and_full_regressions(self):
        common = ci.feature_args('all-optimizations', 'candidate')[1].split(',')
        baseline = common + ['lab-engine/experiment-replay-action-keys']
        candidate = baseline + ['lab-engine/experiment-slot-diff', 'lab-engine/experiment-stats-off-cost']
        self.assertEqual(ci.feature_args(MODE, 'baseline'), ['--features', ','.join(baseline)])
        self.assertEqual(ci.feature_args(MODE, 'candidate'), ['--features', ','.join(candidate)])
        self.assertEqual(set(candidate) - set(baseline),
                         {'lab-engine/experiment-slot-diff', 'lab-engine/experiment-stats-off-cost'})
        for arm, expected in (('baseline', baseline), ('candidate', candidate)):
            commands = ci.build_commands('narrow', MODE, arm)
            self.assertEqual(commands[0][:10], ['cargo', 'test', '--locked', '--release', '-p',
                                               'lab-engine', '-p', 'lab-scenario', '-p', 'lab-search'])
            self.assertEqual(commands[0][-2:], ['--', '--test-threads=1'])
            for command in commands:
                self.assertEqual(command[command.index('--features') + 1], ','.join(expected))
                self.assertNotIn('observe', ' '.join(command))
            with self.assertRaises(ValueError):
                ci.build_commands('smoke', MODE, arm)
        self.assertIn(ci.COMPACT_PROBE_PATH, ci.injected_sources(MODE))

    def test_old_modes_retain_their_baselines_and_candidates(self):
        old4 = ci.feature_args('all-optimizations', 'candidate')
        for mode in (*ci.NEW_MODES, ci.P8DEF_COMBINED):
            self.assertEqual(ci.feature_args(mode, 'baseline'), old4)
        self.assertEqual(ci.feature_args(ci.P8DEF_COMBINED, 'candidate'), ci.feature_args(MODE, 'candidate'))
        for mode in ci.NEW_MODES:
            self.assertEqual(ci.feature_args(mode, 'candidate')[1], old4[1] + ',lab-engine/' + ci.NEW_FEATURES[mode])
        self.assertEqual(ci.feature_args('all-optimizations', 'baseline'), [])

    def test_direct_request_cannot_be_silently_changed_back_to_old4_baseline(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            value = independent.request(root, MODE)
            value['feature_args']['baseline'] = ci.feature_args('p8def-combined', 'baseline')
            independent.write_json(root/'ci-results/request.json', value)
            for operation in (ci.prepare, ci.build):
                with patch.object(ci.subprocess, 'Popen') as processes, self.assertRaises(ValueError):
                    operation(root)
                processes.assert_not_called()


class DirectObserverTests(DirectMode, combined.CombinedObserverTests):
    def test_each_observer_uses_def_runtime_and_only_its_own_diagnostic(self):
        for mode in ci.NEW_MODES:
            commands = ci.new_observer_validation_commands(mode, runtime_selection=MODE)
            for command in commands:
                flags = set(command[command.index('--features') + 1].split(','))
                expected = {'lab-engine/' + name for name in (
                    ci.EXPERIMENT_FEATURE, ci.PREPARED_FEATURE, ci.LEAF_FEATURE,
                    ci.COMPACT_FEATURE, *ci.NEW_FEATURES.values())}
                expected.add(ci.NEW_OBSERVER_TARGETS[mode]['package'] + '/' + ci.NEW_OBSERVERS[mode])
                self.assertEqual(flags, expected)
            if mode == 'stats-off-cost':
                self.assertIn('--lib', commands[0])
                self.assertIn('--exact', commands[0])
        for wrong in ('none', 'slot-diff', 'p11'):
            with self.assertRaises(ValueError):
                ci.new_observer_validation_commands('replay-action-keys', runtime_selection=wrong)


class DirectRepresentationTests(DirectMode, combined.CombinedRepresentationTests):
    def digest(self, root, rows, mode=MODE):
        return independent.SlotRepresentationTests.digest(self, root, rows, mode)

    def test_representation_scope_does_not_exempt_baseline_state_instructions(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            rows = independent.representation_rows()
            initial = self.digest(root, rows)
            next(row for row in rows if row['kind'] == 'state-instructions')['instructions'] = 'changed'
            self.assertNotEqual(initial, self.digest(root, rows))


if __name__ == '__main__':
    unittest.main()
