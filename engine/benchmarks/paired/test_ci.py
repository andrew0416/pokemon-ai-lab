import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import ci


class RequestTests(unittest.TestCase):
    def env(self, root, **changes):
        return {'BASELINE_SHA': 'a'*40, 'CANDIDATE_SHA': '', 'GITHUB_SHA': 'b'*40,
                'SUITE': 'smoke', 'THREADS': '1', 'PAIRS': '2', 'GITHUB_RUN_ID': '123',
                'GITHUB_RUN_ATTEMPT': '1', 'GITHUB_OUTPUT': str(root/'outputs'), **changes}

    def test_blank_candidate_pins_workflow_commit(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            with patch.dict(os.environ, self.env(root), clear=True):
                ci.refs(root)
            metadata = json.loads((root/'ci-results/request.json').read_text())
            self.assertEqual(metadata['candidate_sha'], 'b'*40)
            self.assertIn('candidate_sha='+'b'*40, (root/'outputs').read_text())

    def test_rejects_unsafe_ref_and_unbalanced_or_unbounded_request(self):
        for change in ({'BASELINE_SHA': 'main; touch injected'}, {'PAIRS': '3'},
                       {'PAIRS': '22'}, {'THREADS': '16'}, {'SUITE': '../other'}):
            with self.subTest(change=change), tempfile.TemporaryDirectory() as folder:
                root = Path(folder)
                with patch.dict(os.environ, self.env(root, **change), clear=True):
                    with self.assertRaises(ValueError):
                        ci.refs(root)
                self.assertFalse((root/'outputs').exists())

    def test_rejects_output_directory_reuse(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root/'ci-results').mkdir()
            with patch.dict(os.environ, self.env(root), clear=True):
                with self.assertRaises(FileExistsError):
                    ci.refs(root)


if __name__ == '__main__':
    unittest.main()
