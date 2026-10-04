"""Bounded opt0 logic checks. This is not a performance benchmark."""
from pathlib import Path
import argparse, hashlib, json, os, subprocess

ROOT = Path(__file__).resolve().parent
SOURCE = ROOT / 'source'
OUT = ROOT / 'validation'
OUT.mkdir(exist_ok=True)
parser = argparse.ArgumentParser()
parser.add_argument('--revision', required=True)
parser.add_argument('--phase', choices=['logic', 'build', 'smoke'], required=True)
args = parser.parse_args()
env = os.environ.copy()
env.update(CARGO_TARGET_DIR=str(OUT / 'target'), CARGO_BUILD_JOBS='2',
           CARGO_PROFILE_DEV_DEBUG='0', CARGO_PROFILE_TEST_DEBUG='0',
           CARGO_INCREMENTAL='0', RUSTFLAGS='-C target-cpu=x86-64',
           RUST_MIN_STACK='16777216', PYTHONUTF8='1', PYTHONHASHSEED='0',
           OPENBLAS_NUM_THREADS='1', OMP_NUM_THREADS='1',
           LAB_BUDGET_EVIDENCE=str(OUT / f'quality-grid-{args.revision}.json'))
cargo = ['cargo', '--offline', '--locked', '--manifest-path', 'engine/Cargo.toml', '-p', 'lab-search']
# Cargo options must follow the subcommand.
def command(sub, *extra):
    return [cargo[0], sub, *cargo[1:], *extra]

feature = ['--features', 'experiment-budgeted-search']
binary = OUT / 'target/debug/lab-budget.exe'
scenario = SOURCE / 'engine/oracle/scenarios/eject-button-uturn.json'
checks = {
    'logic': [('unit-on', command('test', *feature, '--lib', '--', '--test-threads=1'), 0, 180)],
    'build': [
        ('clippy-on', command('clippy', *feature, '--lib', '--bin', 'lab-budget', '--', '-D', 'warnings'), 0, 180),
        ('portable-library', command('check', '--no-default-features', *feature, '--lib'), 0, 180),
        ('unit-off', command('test', '--lib', '--', '--test-threads=1'), 0, 180),
        ('cli-build', command('build', *feature, '--bin', 'lab-budget'), 0, 180),
    ],
    'smoke': [
        ('cli-help', [str(binary), '--help'], 0, 30),
        ('cli-no-policy', [str(binary), str(scenario), '--position', '0', '--depth', '1', '--budget', '0', '--rolls', 'median'], 1, 60),
        ('cli-policy', [str(binary), str(scenario), '--position', '0', '--depth', '1', '--budget', '1000', '--rolls', 'median'], 0, 90),
        ('cli-p2', [str(binary), str(scenario), '--position', '0', '--side', 'p2', '--depth', '1', '--budget', '1000', '--rolls', 'median'], 0, 90),
    ],
}[args.phase]
results = []
for name, cmd, expected, timeout in checks:
    path = OUT / f'{args.revision}-{name}.log'
    if path.exists():
        raise SystemExit(f'Preserve existing evidence: {path}')
    print(f'Running {name}', flush=True)
    with path.open('w', encoding='utf-8') as log:
        process = subprocess.Popen(cmd, cwd=SOURCE, env=env, stdout=log,
                                   stderr=subprocess.STDOUT)
        try:
            code, timed_out = process.wait(timeout=timeout), False
        except subprocess.TimeoutExpired:
            # Only this still-owned process and its children. Do not leave a Rust test
            # running after killing its cargo parent, and do not target unrelated builds.
            subprocess.run(['taskkill', '/PID', str(process.pid), '/T', '/F'],
                           stdout=log, stderr=subprocess.STDOUT, timeout=15)
            process.wait(timeout=15)
            code, timed_out = None, True
    text = path.read_text(encoding='utf-8')
    result = {'name': name, 'command': cmd, 'returncode': code, 'expected': expected,
              'timed_out': timed_out, 'passed': code == expected and not timed_out,
              'log': str(path), 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()}
    results.append(result)
    print(json.dumps(result, ensure_ascii=False), flush=True)
    if not result['passed']:
        print(text[-8000:], flush=True)
        break
receipt = {'schema': 1, 'phase': args.phase, 'revision': args.revision,
           'passed': len(results) == len(checks) and all(x['passed'] for x in results),
           'profile': 'opt0 generic x86-64', 'performance_measurement': False, 'checks': results}
(OUT / f'{args.revision}-{args.phase}-receipt.json').write_text(json.dumps(receipt, indent=2), encoding='utf-8')
raise SystemExit(0 if receipt['passed'] else 1)
