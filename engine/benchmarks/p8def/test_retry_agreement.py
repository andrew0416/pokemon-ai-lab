import copy
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import retry_agreement as gate

class PriorAgreementTests(unittest.TestCase):
    def fixture(self):
        run = {'id': gate.RUN, 'head_sha': gate.HEAD, 'repository': {'full_name': gate.REPO},
               'status': 'completed', 'conclusion': 'failure', 'event': 'workflow_dispatch', 'run_attempt': 1}
        jobs = [{'id': key, 'run_id': gate.RUN, 'head_sha': gate.HEAD,
                 'name': name if name.startswith('agreement_') else 'agreement_evaluate (' + name + ', features)',
                 'status': 'completed', 'conclusion': 'success'} for key, name in gate.JOBS.items()]
        return run, {'total_count': len(jobs), 'jobs': jobs}

    def test_exact_agreement_is_valid_despite_unrelated_speed_failure(self):
        self.assertEqual(gate.validate_remote(*self.fixture())['source_sha'], gate.SOURCE)

    def test_missing_failed_duplicate_or_changed_job_rejected(self):
        for mutate in [lambda d: d['jobs'].pop(), lambda d: d['jobs'][0].update(conclusion='failure'),
                       lambda d: d['jobs'][0].update(status='in_progress'),
                       lambda d: d['jobs'][0].update(head_sha='b'*40),
                       lambda d: d['jobs'][0].update(name='unrelated'),
                       lambda d: d['jobs'][0].update(run_id=1),
                       lambda d: d['jobs'].__setitem__(0, copy.deepcopy(d['jobs'][1]))]:
            run, jobs = self.fixture(); mutate(jobs)
            with self.assertRaises(ValueError): gate.validate_remote(run, jobs)

    def test_wrong_run_source_or_attempt_cannot_reuse_agreement(self):
        for key, wrong in [('id', 1), ('head_sha', 'b'*40), ('event', 'push'), ('run_attempt', 2),
                           ('repository', {'full_name': 'unrelated/repo'})]:
            run, jobs = self.fixture(); run[key] = wrong
            with self.assertRaises(ValueError): gate.validate_remote(run, jobs)
        with patch.object(gate, 'SOURCE', 'b'*40), self.assertRaises(ValueError): gate.validate_source()

    def test_replaced_summary_fails_before_acceptance(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'summary.json'; path.write_text('{"complete":true}')
            with self.assertRaises(ValueError): gate.validate_summary(path)

if __name__ == '__main__': unittest.main()
