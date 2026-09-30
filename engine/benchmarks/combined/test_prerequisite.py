import unittest
from prerequisite import HEAD_SHA, REPOSITORY, RUN_ID, validate


class PrerequisiteTests(unittest.TestCase):
    def setUp(self):
        self.run = {'id': RUN_ID, 'head_sha': HEAD_SHA, 'repository': {'full_name': REPOSITORY},
                    'event': 'workflow_dispatch', 'status': 'completed', 'conclusion': 'success',
                    'html_url': f'https://github.com/{REPOSITORY}/actions/runs/{RUN_ID}'}

    def test_exact_success(self):
        self.assertEqual(validate(self.run)['id'], RUN_ID)

    def test_other_or_incomplete_run_is_rejected(self):
        for key, wrong in [('id', RUN_ID + 1), ('head_sha', '0' * 40),
                           ('repository', {'full_name': 'other/repo'}), ('event', 'push'),
                           ('status', 'in_progress'), ('conclusion', 'failure'),
                           ('conclusion', None), ('conclusion', 'cancelled')]:
            with self.subTest(key=key, value=wrong), self.assertRaises(ValueError):
                validate({**self.run, key: wrong})

    def test_missing_fields_are_rejected(self):
        for key in ('id', 'head_sha', 'repository', 'event', 'status', 'conclusion'):
            with self.subTest(key=key), self.assertRaises(ValueError):
                validate({k: v for k, v in self.run.items() if k != key})


if __name__ == '__main__':
    unittest.main()
