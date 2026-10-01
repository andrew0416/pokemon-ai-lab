"""Observer-OFF paired controls and censored tail; correctness gates always run first."""
import argparse
import os
from pathlib import Path

import ci
import compare_public
from common import (c, base_ci, binding, features, fixed_case, comparison, timed_pair,
                    tail_comparison, control_summary, PLAN_SHA, CONTROL_CASES, TAIL_CASE, PAIR_COUNT)
import process_run
import diagnostic_process

def execute(workspace, receipt, case_id, arm, directory, cpu, tail=False):
    case, original = fixed_case(workspace, case_id)
    env = ci.environment(workspace, arm)
    c.require(not any(key.startswith('LAB_') for key in env), 'Benchmark child must have no instrumentation environment')
    proof = receipt['arms'][arm]
    binary = workspace / ('target-p1e-' + arm) / 'release/lab-distribution-bench'
    c.require(str(binary) == proof['binary'] and c.sha(binary) == proof['binary_sha256'], 'Arm binary changed')
    scenario = c.safe_file(workspace / 'controller', case['scenario'])
    directory.mkdir(parents=True, exist_ok=False)
    entry = {'case_id': case_id, 'arm': arm, 'binary_sha256': proof['binary_sha256'],
             'observer_enabled': False, 'status': 'preparing'}
    path = directory / 'receipt.json'
    try:
        for phase in ('description', 'measurement'):
            c.require(c.sha(binary) == proof['binary_sha256'] and c.sha(scenario) == case['scenario_sha256'], 'Binary/scenario changed before child')
            c.require(c.sha(original) == PLAN_SHA[case_id], 'Frozen plan changed before child')
            argv = [str(binary), str(scenario), '--joint-seed', str(case['joint_seed'])]
            stem = directory / phase
            row = {'status': 'running', 'lab_environment': {}}
            entry[phase] = row
            base_ci.write(path, entry)
            if phase == 'description':
                argv += ['--describe']
                runner = process_run
            else:
                plan = directory / 'description.stdout'
                c.require(c.sha(plan) == PLAN_SHA[case_id], 'Fresh description differs from original')
                argv += ['--plan', str(plan), '--sample-seeds', ','.join(map(str, case['sample_seeds']))]
                runner = diagnostic_process if tail else process_run
            process = runner.run(argv, ci.source_root(workspace, arm) / 'engine', env, stem, cpu=cpu)
            row.update(status=process['status'], process=process)
            for channel in ('stdout', 'stderr'):
                raw = stem.with_suffix('.' + channel)
                row[channel + '_sha256'] = c.sha(raw)
                row[channel + '_bytes'] = raw.stat().st_size
            stderr = stem.with_suffix('.stderr').read_bytes()
            c.require(b'P17_FRONTIER ' not in stderr and b'lab-engine: factored stage' not in stderr,
                      'Observer unexpectedly active')
            if process['status'] != 'ok':
                entry['status'] = process['status']
                return entry
            value = c.line(stem.with_suffix('.stdout'))
            if phase == 'description':
                c.description(value, case)
                c.require(stem.with_suffix('.stdout').read_bytes() == original.read_bytes(), 'Fresh description bytes changed')
            else:
                c.result(value, c.line(original), case)
                entry.update(result=value, status='ok')
        return entry
    except Exception as error:
        entry.update(status='validation_or_execution_error', error=f'{type(error).__name__}: {error}')
        return entry
    finally:
        base_ci.write(path, entry)

def verify_build(workspace, bound):
    folder = workspace / ci.RESULTS
    receipt = c.strict_json((folder / 'build-receipt.json').read_bytes())
    c.require(receipt['status'] == 'success' and receipt['cached_regression_reused'] is False
              and receipt['source_sha'] == bound['source_sha'] and receipt['corpus_sha256'] == c.CORPUS_SHA,
              'Fresh bound build required')
    c.require(set(receipt['arms']) == set(ci.ARMS), 'Missing original/OFF/ON arm')
    expected_commands = []
    for arm in ci.ARMS:
        c.require(receipt['arms'][arm]['features'] == features(arm == 'on'), 'Unexpected compiled feature set')
        c.require(ci.fingerprints(workspace, arm, bound) == receipt['arms'][arm]['compiler_features'], 'Compiled fingerprint changed after build')
        for label, argv, tests, filtered in ci.command_plan(arm, bound):
            expected_commands.append((arm, label, argv, tests, filtered))
        public = folder / (arm + '-public-records.jsonl')
        c.require(c.sha(public) == receipt['arms'][arm]['public_records_sha256'], 'Public records changed')
    c.require(len(receipt['commands']) == len(expected_commands), 'Fresh command list incomplete')
    for row, (arm, label, argv, tests, filtered) in zip(receipt['commands'], expected_commands):
        c.require((row['arm'], row['label'], row['argv'], row['returncode']) == (arm, label, argv, 0), 'Fresh command identity mismatch')
        log = folder / (arm + '-' + label + '.log')
        c.require(row['log'] == log.name and c.sha(log) == row['log_sha256'], 'Fresh test/build log changed')
        if tests:
            c.require(ci.named_test_proof(log.read_text(), tests, filtered) == row['test_proof'], 'Fresh named-test proof mismatch')
    agreement = {arm + '_vs_on': compare_public.compare_records(folder / (arm + '-public-records.jsonl'), folder / 'on-public-records.jsonl')
                 for arm in ('original', 'off')}
    c.require(agreement == receipt['public_agreement'] and all(proof['passed'] is True for proof in agreement.values())
              and receipt['public_comparator_sha256'] == bound['public_comparator_sha256'], 'Public correctness agreement changed')
    c.require(c.sha(folder / 'public-agreement.json') == receipt['public_agreement_sha256'], 'Public agreement receipt changed')
    return receipt

def control_order(repeat, case_index):
    c.require(type(repeat) is int and 0 <= repeat < PAIR_COUNT and case_index in range(len(CONTROL_CASES)), 'Bad pair ordinal')
    offset = (repeat + case_index) % 3
    return ci.ARMS[offset:] + ci.ARMS[:offset]

def run(workspace):
    folder = workspace / ci.RESULTS
    path = folder / 'comparison-summary.json'
    c.require(not path.exists(), 'Comparison already attempted; no silent retry')
    record = {'schema': 1, 'status': 'preparing', 'official_full500_metrics': None, 'full500_complete': False,
              'observer_enabled': False, 'controls': [], 'tail': {},
              'public_correctness_scope': 'Ten bounded synthetic full-State/Suspension fixtures plus named inherited regression tests; not global proof.',
              'performance_scope': 'Primary original a4 versus complete candidate ON includes all implementation changes; secondary candidate OFF versus ON isolates its feature. Same runner/core6/compiler. API kernel and whole-child CPU/RSS are separate.'}
    base_ci.write(path, record)
    try:
        bound = binding()
        ci.verify_source(workspace / 'source', bound)
        ci.verify_original(workspace / 'original', bound)
        receipt = verify_build(workspace, bound)
        record.update(source_sha=bound['source_sha'], corpus_sha256=c.CORPUS_SHA,
                      correctness_gate_passed=True, public_agreement=receipt['public_agreement'])
        cpu = min(os.sched_getaffinity(0))
        record['measurement_cpu'] = cpu
        for repeat in range(PAIR_COUNT):
            for case_index, case_id in enumerate(CONTROL_CASES):
                row = {'case_id': case_id, 'repeat': repeat, 'order': list(control_order(repeat, case_index)), 'arms': {}}
                record['controls'].append(row)
                for arm in row['order']:
                    row['arms'][arm] = execute(workspace, receipt, case_id, arm, folder / 'controls' / f'pair-{repeat}' / case_id / arm, cpu)
                    base_ci.write(path, record)
                    c.require(row['arms'][arm]['status'] == 'ok', 'Bounded control failed: ' + case_id + ' ' + arm)
                case, plan = fixed_case(workspace, case_id)
                row['timing'] = timed_pair(row['arms']['original'], row['arms']['on'], c.line(plan), case)
                row['baseline_arm'] = 'original'
                row['feature_only_timing'] = timed_pair(row['arms']['off'], row['arms']['on'], c.line(plan), case)
                base_ci.write(path, record)
        record['control_summary'] = control_summary(record['controls'])
        # Timeouts are expected censored outcomes, so OFF timeout must not prevent ON execution.
        for arm in ('original', 'on'):
            entry = execute(workspace, receipt, TAIL_CASE, arm, folder / 'tail' / TAIL_CASE / arm, cpu, tail=True)
            record['tail'][arm] = entry
            base_ci.write(path, record)
            c.require(entry['status'] in ('ok', 'timeout'), 'Tail child failed validation/resources/execution: ' + arm)
            c.require('measurement' in entry and entry['description']['status'] == 'ok', 'Tail description failed; not a measurement censor')
        case, plan = fixed_case(workspace, TAIL_CASE)
        record['tail_baseline_arm'] = 'original'
        record['tail_comparison'] = tail_comparison(record['tail']['original'], record['tail']['on'], c.line(plan), case)
        record['status'] = 'completed_censored_tail' if record['tail_comparison']['censored'] else 'completed'
        record['experiment_complete'] = True
        record['tail_reference_complete_in_both_arms'] = not record['tail_comparison']['censored']
        return 0
    except Exception as error:
        record.update(status='validation_or_execution_error', experiment_complete=False,
                      error=f'{type(error).__name__}: {error}')
        return 1
    finally:
        base_ci.write(path, record)

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--workspace', required=True, type=Path)
    args = parser.parse_args()
    raise SystemExit(run(args.workspace.resolve()))

if __name__ == '__main__':
    main()
