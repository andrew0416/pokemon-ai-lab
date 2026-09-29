"""Small CI preparation driver. No shell interpolation of dispatch inputs."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def output(argv, cwd=None):
    return subprocess.check_output(argv, cwd=cwd, text=True).strip()


def refs(workspace):
    result = workspace / 'ci-results'
    result.mkdir(exist_ok=False)
    baseline = os.environ['BASELINE_SHA']
    candidate = os.environ.get('CANDIDATE_SHA') or os.environ['GITHUB_SHA']
    for value in (baseline, candidate):
        if not re.fullmatch('[0-9a-fA-F]{40}', value):
            raise ValueError('Use a complete 40-character commit SHA from this repository.')
    suite = os.environ['SUITE']
    threads = int(os.environ['THREADS'])
    pairs = int(os.environ['PAIRS'])
    if suite not in ('smoke', 'narrow') or threads not in (1, 2, 4):
        raise ValueError('Invalid suite or thread count')
    if pairs < 2 or pairs > 20 or pairs % 2:
        raise ValueError('pairs must be even, from 2 through 20')
    cpus = len(os.sched_getaffinity(0)) if hasattr(os, 'sched_getaffinity') else os.cpu_count()
    if threads > cpus:
        raise ValueError('Requested threads exceed CPUs available to this runner')
    metadata = {'baseline_sha': baseline.lower(), 'candidate_sha': candidate.lower(),
                'workflow_sha': os.environ['GITHUB_SHA'], 'suite': suite, 'threads': threads,
                'pairs': pairs, 'run_id': os.environ['GITHUB_RUN_ID'],
                'run_attempt': os.environ['GITHUB_RUN_ATTEMPT'], 'available_cpus': cpus}
    (result/'request.json').write_text(json.dumps(metadata, indent=2)+'\n', encoding='utf-8')
    with open(os.environ['GITHUB_OUTPUT'], 'a', encoding='utf-8') as stream:
        for key in ('baseline_sha', 'candidate_sha'):
            stream.write(f'{key}={metadata[key]}\n')


def prepare(workspace):
    result = workspace/'ci-results'
    metadata = json.loads((result/'request.json').read_text(encoding='utf-8'))
    harness = Path(__file__).with_name('harness.rs')
    metadata['harness_sha256'] = sha(harness)
    metadata['rustc'] = output(['rustc', '-Vv'])
    metadata['cargo'] = output(['cargo', '-V'])
    metadata['build_environment'] = {key: value for key, value in sorted(os.environ.items())
                                     if key.startswith('CARGO_PROFILE_RELEASE_') or key in
                                     ('RUSTFLAGS', 'CARGO_INCREMENTAL', 'CARGO_BUILD_JOBS')}
    metadata['sources'] = {}
    for label in ('baseline', 'candidate'):
        root = workspace/label
        actual = output(['git', 'rev-parse', 'HEAD'], root)
        if actual != metadata[f'{label}_sha']:
            raise ValueError(f'{label} checkout does not match requested SHA')
        if output(['git', 'status', '--porcelain'], root):
            raise ValueError(f'{label} checkout is not clean before harness injection')
        destination = root/'engine/search/examples/ci_bench.rs'
        if destination.exists():
            raise ValueError('Reserved example ci_bench.rs already exists in source revision')
        metadata['sources'][label] = {
            'commit': actual, 'lock_sha256': sha(root/'engine/Cargo.lock'),
            'workspace_manifest_sha256': sha(root/'engine/Cargo.toml'),
            'search_manifest_sha256': sha(root/'engine/search/Cargo.toml'),
            'package_manifests': {name: sha(root/'engine'/name/'Cargo.toml')
                                  for name in ('core', 'scenario', 'py')},
            'cargo_configuration': {name: sha(root/name) for name in (
                '.cargo/config', '.cargo/config.toml', 'rust-toolchain', 'rust-toolchain.toml',
                'engine/.cargo/config', 'engine/.cargo/config.toml',
                'engine/rust-toolchain', 'engine/rust-toolchain.toml') if (root/name).is_file()}}
    # A dependency/profile change needs a separately designed experiment.
    for key in ('lock_sha256', 'workspace_manifest_sha256', 'search_manifest_sha256',
                'package_manifests', 'cargo_configuration'):
        if metadata['sources']['baseline'][key] != metadata['sources']['candidate'][key]:
            raise ValueError(f'Baseline/candidate differ in {key}; strict source-only benchmark refused')
    for label in ('baseline', 'candidate'):
        destination = workspace/label/'engine/search/examples/ci_bench.rs'
        destination.parent.mkdir(exist_ok=True)
        shutil.copyfile(harness, destination)
    (result/'provenance.json').write_text(json.dumps(metadata, indent=2)+'\n', encoding='utf-8')


def build(workspace):
    # Finish ALL tests/builds before any benchmark process is launched.
    request = json.loads((workspace/'ci-results/request.json').read_text(encoding='utf-8'))
    tests = [['cargo', 'test', '--locked', '--release', '-p', 'lab-engine', '-p', 'lab-scenario', '-p', 'lab-search']]
    if request['suite'] == 'smoke':
        # Infrastructure checks need the harness and its fixture, not every oracle binary.
        # Actual candidate comparisons (narrow) retain the full regression gate above.
        tests = [
            ['cargo', 'test', '--locked', '--release', '-p', 'lab-engine', '-p', 'lab-search', '--lib'],
            ['cargo', 'test', '--locked', '--release', '-p', 'lab-scenario', '--test', 'abilities_slow_start_truant'],
        ]
    (workspace/'ci-results/test-plan.json').write_text(
        json.dumps({'suite': request['suite'], 'commands_per_version': tests}, indent=2)+'\n', encoding='utf-8')
    for label in ('baseline', 'candidate'):
        env = os.environ.copy()
        env['CARGO_TARGET_DIR'] = str(workspace/('target-'+label))
        commands = tests + [
            ['cargo', 'build', '--locked', '--release', '-p', 'lab-search', '--example', 'ci_bench'],
        ]
        with (workspace/'ci-results'/f'{label}-build.log').open('w', encoding='utf-8') as log:
            for argv in commands:
                print(f'{label}: {" ".join(argv)}', flush=True)
                log.write('COMMAND '+json.dumps(argv)+'\n')
                log.flush()
                proc = subprocess.Popen(argv, cwd=workspace/label/'engine', env=env,
                                        stdout=log, stderr=subprocess.STDOUT,
                                        start_new_session=(os.name == 'posix'))
                try:
                    proc.wait(timeout=1800)
                except subprocess.TimeoutExpired:
                    if os.name == 'posix':
                        os.killpg(proc.pid, signal.SIGKILL)
                    else:
                        proc.kill()
                    proc.wait()
                    raise RuntimeError(f'{label} build/test timed out; process group terminated')
                if proc.returncode:
                    log.flush()
                    print((workspace/'ci-results'/f'{label}-build.log').read_text(encoding='utf-8')[-12000:])
                    raise RuntimeError(f'{label} build/test failed ({proc.returncode}); see artifact log')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('stage', choices=('refs', 'prepare', 'build'))
    parser.add_argument('--workspace', type=Path, required=True)
    args = parser.parse_args()
    try:
        globals()[args.stage](args.workspace.resolve())
    except Exception as error:
        result = args.workspace/'ci-results'
        result.mkdir(exist_ok=True)
        (result/(args.stage+'-error.txt')).write_text(f'{type(error).__name__}: {error}\n', encoding='utf-8')
        raise


if __name__ == '__main__':
    main()
