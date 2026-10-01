"""Original engine remains byte-equivalent except the explicit public test registration."""
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import ci

class OriginalPreservationTests(unittest.TestCase):
    def test_original_allows_only_identical_public_test_and_manifest_append(self):
        for fault in (None, 'runtime', 'extra', 'manifest', 'public', 'head', 'parent'):
            with tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                manifest = root / ci.MANIFEST_PATH
                manifest.parent.mkdir(parents=True)
                baseline = b'[package]\nname = "lab-scenario"\n'
                manifest.write_bytes(baseline + ci.TEST_REGISTRATION.encode() + (b'# changed\n' if fault == 'manifest' else b''))
                public = root / ci.PUBLIC_PATH
                public.parent.mkdir(parents=True)
                public.write_bytes(b'public source')
                digest = ci.c.sha(public)
                if fault == 'public':
                    public.write_bytes(b'changed public source')
                bench = root / ci.BENCHMARK
                bench.parent.mkdir(parents=True)
                bench.write_bytes(b'frozen benchmark')
                def command(argv, cwd):
                    self.assertEqual(cwd, root)
                    if argv == ['git', 'rev-parse', 'HEAD']:
                        return 'bad' if fault == 'head' else ci.SOURCE_PARENT
                    if argv == ['git', 'rev-parse', 'HEAD^']:
                        return 'bad' if fault == 'parent' else ci.c.CANONICAL_SOURCE
                    if argv == ['git', 'diff', '--name-only', 'HEAD']:
                        return ci.MANIFEST_PATH + ('\nengine/core/src/turn/lazy.rs' if fault == 'runtime' else '')
                    if argv == ['git', 'ls-files', '--others', '--exclude-standard']:
                        return ci.PUBLIC_PATH + ('\nrogue.rs' if fault == 'extra' else '')
                    raise AssertionError(argv)
                with (patch.object(ci.base_ci, 'command', side_effect=command),
                      patch.object(ci.subprocess, 'check_output', return_value=baseline),
                      patch.object(ci, 'BENCHMARK_SHA', ci.c.sha(bench))):
                    if fault:
                        with self.assertRaises(ValueError):
                            ci.verify_original(root, {'changed_file_sha256': {ci.PUBLIC_PATH: digest}})
                    else:
                        value = ci.verify_original(root, {'changed_file_sha256': {ci.PUBLIC_PATH: digest}})
                        self.assertTrue(value['runtime_unchanged'])

    def test_original_command_plan_excludes_candidate_private_tests_and_feature(self):
        bound = {'public_tests': ['p1e_full_state_distributions_match_flat_and_rollback'],
                 'core_suites': [{'label': 'core_p1e', 'arms': ['off', 'on'], 'filter': 'turn::p1e', 'tests': ['turn::p1e::example']}]}
        commands = ci.command_plan('original', bound)
        self.assertEqual([row[0] for row in commands], ['harness_tests', 'public_tests', 'factored_tests', 'harness_build'])
        for _, argv, _, _ in commands:
            self.assertNotIn(ci.FEATURE, ','.join(argv))
            self.assertNotIn('observer', ','.join(argv))

if __name__ == '__main__':
    unittest.main()
