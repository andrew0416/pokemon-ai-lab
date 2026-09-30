"""Exact-key reuse of a verified executable and regression proof, never Cargo targets.

All cache failures fall back before creating a target. Actual build/test failures are owned
by ci.py and are never retried here. No recursive deletion or remote cache deletion is used.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import platform
import re
import shutil
import stat
import subprocess
import sys
import tomllib
import uuid

SCHEMA_VERSION = 1
TRUSTED_REPOSITORY = 'andrew0416/pokemon-ai-lab'
TRUSTED_DEFAULT_BRANCH = 'lab-engine'
LABELS = ('baseline', 'candidate')
KEY_PREFIX = 'verified-build-v1-'
CONTROLLER_FILES = ('ci.py', 'build_cache.py', 'dependency_target.py', 'harness.rs',
                    'run.py', 'suites.json', 'memory.py')
COMPACT_CONTROLLER_FILES = ('compact_probe.py', 'compact_probe.rs')
COMPACT_PROBE_PATH = 'engine/scenario/examples/ci_compact_probe.rs'
MAX_BUNDLE_FILES = 1000


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=True).encode()


def digest(path):
    value = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            value.update(block)
    return value.hexdigest()


def recipe_key(recipe):
    return KEY_PREFIX + hashlib.sha256(canonical(recipe)).hexdigest()


def _label(label):
    if label not in LABELS:
        raise ValueError('Invalid cache build label')


def _command(argv, cwd=None):
    return subprocess.check_output(argv, cwd=cwd, text=True, stderr=subprocess.STDOUT,
                                   timeout=30).strip()


def _file_identity(path):
    path = path.resolve(strict=True)
    if not path.is_file():
        raise ValueError(f'Not a regular identity file: {path}')
    return {'path': str(path), 'size': path.stat().st_size, 'sha256': digest(path)}


def _tree_files(root):
    """An exact regular-file inventory without following directory links."""
    if root.is_symlink() or not root.is_dir():
        raise ValueError('Artifact root must be a regular directory')
    result = {}
    for folder, directories, files in os.walk(root, followlinks=False):
        for name in directories + files:
            path = Path(folder)/name
            if path.is_symlink() or (hasattr(path, 'is_junction') and path.is_junction()):
                raise ValueError(f'Artifact links are forbidden: {path}')
            mode = path.lstat().st_mode
            if name in files:
                if not stat.S_ISREG(mode):
                    raise ValueError(f'Artifact is not a regular file: {path}')
                result[path.relative_to(root).as_posix()] = path
            elif not stat.S_ISDIR(mode):
                raise ValueError(f'Artifact is not a directory: {path}')
    if len(result) > MAX_BUNDLE_FILES:
        raise ValueError('Artifact inventory exceeds the bounded bundle size')
    return result


def _relative(value):
    if not isinstance(value, str) or not value or '\\' in value:
        raise ValueError('Invalid artifact path')
    path = PurePosixPath(value)
    if path.is_absolute() or any(part in ('', '.', '..') for part in path.parts):
        raise ValueError('Artifact path must be relative and normalized')
    if path.as_posix() != value or any(':' in part for part in path.parts):
        raise ValueError('Invalid artifact path component')
    return path


def _environment():
    exact = ('RUSTFLAGS', 'RUSTDOCFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'CARGO_INCREMENTAL',
             'CARGO_BUILD_JOBS', 'CARGO_BUILD_TARGET', 'RUSTUP_TOOLCHAIN', 'RUSTUP_HOME',
             'CARGO_HOME', 'CC', 'CXX', 'AR', 'LD', 'CFLAGS', 'CXXFLAGS', 'LDFLAGS',
             'PATH', 'LD_LIBRARY_PATH', 'LIBRARY_PATH', 'LAB_ENGINE_FACTORED',
             'PYTHONHASHSEED', 'OPENBLAS_NUM_THREADS', 'OMP_NUM_THREADS')
    return {key: value for key, value in sorted(os.environ.items())
            if key in exact or key.startswith(('CARGO_PROFILE_', 'CARGO_TARGET_'))
            and key != 'CARGO_TARGET_DIR'}


def _runtime_identity(cwd=None):
    if platform.system() != 'Linux' or platform.machine() != 'x86_64':
        raise ValueError('Verified build reuse is limited to the Linux x86_64 runner')
    if os.environ.get('RUSTFLAGS') != '-Ctarget-cpu=x86-64':
        raise ValueError('Cache requires the exact generic x86-64 RUSTFLAGS')
    overrides = ('CARGO_ENCODED_RUSTFLAGS', 'RUSTDOCFLAGS', 'RUSTC', 'RUSTDOC',
                 'CARGO_BUILD_RUSTC', 'CARGO_BUILD_RUSTDOC', 'CC', 'CXX', 'AR', 'LD',
                 'CFLAGS', 'CXXFLAGS', 'LDFLAGS')
    if any(os.environ.get(name) for name in overrides):
        raise ValueError('Custom compiler, linker or flags are outside the cache contract')
    if any(value for key, value in os.environ.items()
           if key.startswith('CARGO_TARGET_') and key.endswith(('RUSTFLAGS', 'LINKER', 'RUNNER'))):
        raise ValueError('Custom target tools or flags are outside the cache contract')
    if any(value for key, value in os.environ.items() if 'WRAPPER' in key):
        raise ValueError('Compiler wrappers are outside the cache contract')
    image = {name: os.environ.get(name) for name in ('ImageOS', 'ImageVersion')}
    if not all(image.values()):
        raise ValueError('Immutable hosted runner image identity is unavailable')
    tools = {}
    versions = {}
    for name, args in (('rustc', ['-Vv']), ('cargo', ['-V']), ('cc', ['--version']),
                       ('ld', ['--version']), ('ldd', ['--version'])):
        executable = shutil.which(name)
        if not executable:
            raise ValueError(f'Missing identity tool: {name}')
        tools[name] = _file_identity(Path(executable))
        versions[name] = _command([executable, *args], cwd=cwd)
    host_match = re.search(r'^host: (\S+)$', versions['rustc'], re.MULTILINE)
    if not host_match:
        raise ValueError('rustc host identity is unavailable')
    host = host_match.group(1)
    if os.environ.get('CARGO_BUILD_TARGET') not in (None, '', host):
        raise ValueError('Cross compilation is outside the cache contract')
    sysroot = Path(_command(['rustc', '--print', 'sysroot'], cwd=cwd))
    libdir = Path(_command(['rustc', '--print', 'target-libdir'], cwd=cwd))
    linker_name = _command(['cc', '-print-prog-name=ld'], cwd=cwd)
    linker_path = Path(linker_name) if Path(linker_name).is_absolute() else Path(
        shutil.which(linker_name) or linker_name)
    linker = _file_identity(linker_path)
    compiler_target = _command(['cc', '-dumpmachine'], cwd=cwd)
    libraries = {p.name: _file_identity(p) for p in sorted(libdir.iterdir()) if p.is_file()}
    if not libraries:
        raise ValueError('Rust target libraries are unavailable')
    # Read only the trusted system linker registry. Never run ldd on a restored executable.
    ldconfig = shutil.which('ldconfig') or '/sbin/ldconfig'
    registry = _command([ldconfig, '-p'])
    abi = {}
    for name in ('libc.so.6', 'libgcc_s.so.1', 'libm.so.6', 'ld-linux-x86-64.so.2'):
        matches = re.findall(r'^\s*' + re.escape(name) +
                             r'\s+\([^\n]*x86-64[^\n]*\) => (\S+)$',
                             registry, re.MULTILINE)
        if not matches:
            raise ValueError(f'Missing system ABI identity: {name}')
        abi[name] = [_file_identity(Path(value)) for value in sorted(set(matches))]
    return {'os': platform.system(), 'arch': platform.machine(), 'image': image,
            'host': host, 'sysroot': str(sysroot), 'target_libdir': str(libdir),
            'compiler': _file_identity(sysroot/'bin/rustc'), 'tools': tools,
            'versions': versions, 'linker': linker, 'compiler_target': compiler_target,
            'rust_libraries': libraries, 'system_abi': abi,
            'environment': _environment()}


def _cargo_configuration(root):
    paths = set()
    for parent in (root/'engine', *(root/'engine').parents):
        for name in ('config', 'config.toml'):
            path = parent/'.cargo'/name
            if path.is_file():
                paths.add(path)
    cargo_home = Path(os.environ.get('CARGO_HOME', Path.home()/'.cargo'))
    for name in ('config', 'config.toml'):
        if (cargo_home/name).is_file():
            paths.add(cargo_home/name)
    result = {}
    for path in sorted(paths):
        data = tomllib.loads(path.read_text(encoding='utf-8'))

        def reject_overrides(value):
            if isinstance(value, dict):
                for key, nested in value.items():
                    if key in ('rustflags', 'rustdocflags', 'rustc', 'rustc-wrapper',
                               'rustc-workspace-wrapper', 'linker', 'runner') and nested:
                        raise ValueError(f'Custom Cargo tool/flags configuration: {path}')
                    reject_overrides(nested)
        reject_overrides(data)
        result[str(path)] = _file_identity(path)
    return result


def make_recipe(workspace, label):
    """Recompute actual build inputs; no run id, timestamp, pairs or timing results."""
    import ci
    import run

    _label(label)
    workspace = Path(workspace).resolve()
    request = json.loads((workspace/'ci-results/request.json').read_text(encoding='utf-8'))
    root = workspace/label
    source_sha = _command(['git', 'rev-parse', 'HEAD'], root)
    if source_sha != request[label + '_sha']:
        raise ValueError('Source checkout does not match the requested commit')
    if _command(['git', 'status', '--porcelain', '--untracked-files=no'], root):
        raise ValueError('Tracked source changed since preparation')
    untracked = subprocess.check_output(
        ['git', 'ls-files', '--others', '--exclude-standard', '-z'], cwd=root, timeout=30)
    compact = request['candidate_feature'] in ('compact-volatiles', 'all-optimizations', *ci.STRICT_MODES)
    allowed_untracked = set(ci.injected_sources(request['candidate_feature']))
    if set(filter(None, untracked.decode('utf-8').split('\0'))) - allowed_untracked:
        raise ValueError('Source contains unexpected untracked files')
    names = subprocess.check_output(['git', 'ls-files', '-z'], cwd=root, timeout=30)
    files = {}
    for name in names.decode('utf-8').split('\0'):
        if not name:
            continue
        _relative(name)
        path = root/name
        if path.is_symlink() or not path.is_file() or not path.resolve().is_relative_to(root.resolve()):
            raise ValueError(f'Unsupported tracked source type: {name}')
        files[name] = digest(path)
    harness = root/'engine/search/examples/ci_bench.rs'
    controller = Path(__file__).resolve().parent
    controller_files = CONTROLLER_FILES + (COMPACT_CONTROLLER_FILES if compact else ())
    if request['candidate_feature'] == ci.BORROWED_MODE:
        controller_files += ('borrowed_child_keys.py', 'borrowed_child_keys_probe.rs')
    if request['candidate_feature'] == ci.MATRIX_MODE:
        controller_files += ('matrix_pass_through.py', 'matrix_pass_through_probe.rs')
    if request['candidate_feature'] == ci.PL_MODE:
        controller_files += ('prepared_leaf.py', 'prepared_leaf_probe.rs')
    if request['candidate_feature'] == ci.R1_MATRIX_MODE:
        controller_files += ('r1_matrix_pass_through.py', 'r1_matrix_pass_through_probe.rs', 'matrix_pass_through.py', 'borrowed_child_keys.py')
    driver = {name: digest(controller/name) for name in controller_files}
    workflow = controller.parents[2]/'.github/workflows/engine-benchmark.yml'
    driver['.github/workflows/engine-benchmark.yml'] = digest(workflow)
    _, inputs = run.load_cases(request['suite'], root=controller.parents[2])
    shared_inputs = {path.relative_to(controller.parents[2]).as_posix(): digest(path)
                     for path in inputs}
    if digest(harness) != driver['harness.rs']:
        raise ValueError('Injected harness differs from the controller source')
    probe_identity = {}
    if compact:
        probe = root/COMPACT_PROBE_PATH
        if (probe.is_symlink() or not probe.is_file()
                or not probe.resolve().is_relative_to(root.resolve())
                or digest(probe) != driver['compact_probe.rs']):
            raise ValueError('Injected compact probe differs from the controller source')
        probe_identity = {'compact_probe_sha256': digest(probe)}
    if request['candidate_feature'] == ci.BORROWED_MODE:
        from borrowed_child_keys import PROBE_PATH
        probe = root/PROBE_PATH
        if (probe.is_symlink() or not probe.is_file()
                or not probe.resolve().is_relative_to(root.resolve())
                or digest(probe) != driver['borrowed_child_keys_probe.rs']):
            raise ValueError('Injected P13 observer probe differs from controller source')
        probe_identity['borrowed_child_keys_probe_sha256'] = digest(probe)
    if request['candidate_feature'] == ci.MATRIX_MODE:
        from matrix_pass_through import PROBE_PATH
        probe = root/PROBE_PATH
        if (probe.is_symlink() or not probe.is_file()
                or not probe.resolve().is_relative_to(root.resolve())
                or digest(probe) != driver['matrix_pass_through_probe.rs']):
            raise ValueError('Injected P14 observer probe differs from controller source')
        probe_identity['matrix_pass_through_probe_sha256'] = digest(probe)
    if request['candidate_feature'] == ci.PL_MODE:
        from prepared_leaf import PROBE_PATH
        probe = root/PROBE_PATH
        if (probe.is_symlink() or not probe.is_file()
                or not probe.resolve().is_relative_to(root.resolve())
                or digest(probe) != driver['prepared_leaf_probe.rs']):
            raise ValueError('Injected P15 observer probe differs from controller source')
        probe_identity['prepared_leaf_probe_sha256'] = digest(probe)
    if request['candidate_feature'] == ci.R1_MATRIX_MODE:
        from r1_matrix_pass_through import PROBE_PATH
        probe = root/PROBE_PATH
        if (probe.is_symlink() or not probe.is_file()
                or not probe.resolve().is_relative_to(root.resolve())
                or digest(probe) != driver['r1_matrix_pass_through_probe.rs']):
            raise ValueError('Injected R1/P14 observer probe differs from controller source')
        probe_identity['r1_matrix_probe_sha256'] = digest(probe)
    packages, expected, hurt_active = ci.fingerprint_expectations(
        request['candidate_feature'], label)
    return {'schema_version': SCHEMA_VERSION, 'label': label, 'suite': request['suite'],
            'selection': request['candidate_feature'],
            'source': {'sha': source_sha, 'tracked_files_sha256': hashlib.sha256(canonical(files)).hexdigest(),
                       'tracked_file_count': len(files), 'harness_sha256': digest(harness),
                       **probe_identity,
                       'lock_sha256': digest(root/'engine/Cargo.lock'),
                       'cargo_configuration': _cargo_configuration(root)},
            'commands': ci.build_commands(request['suite'], request['candidate_feature'], label),
            'fingerprint_spec': {'packages': {p: list(v) for p, v in packages.items()},
                                 'expected': expected, 'hurt_active': hurt_active},
            'identity': {'runtime': _runtime_identity(cwd=root/'engine'), 'build_driver': driver,
                         'shared_input_sha256': shared_inputs,
                         'repository': os.environ.get('GITHUB_REPOSITORY'),
                         'default_branch': os.environ.get('BUILD_CACHE_DEFAULT_BRANCH')}}


def _write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True)+'\n', encoding='utf-8')


def _github_output(name, value):
    if os.environ.get('GITHUB_OUTPUT'):
        with open(os.environ['GITHUB_OUTPUT'], 'a', encoding='utf-8') as stream:
            stream.write(f'{name}={value}\n')


def plan(workspace):
    workspace = Path(workspace).resolve()
    result = workspace/'ci-results/cache-plan.json'
    try:
        if os.environ.get('BUILD_CACHE_ENABLED') != '1':
            raise ValueError('Build cache is disabled')
        if os.environ.get('GITHUB_REPOSITORY') != TRUSTED_REPOSITORY:
            raise ValueError('Repository is outside the trusted cache scope')
        versions = {}
        for label in LABELS:
            recipe = make_recipe(workspace, label)
            versions[label] = {'key': recipe_key(recipe), 'recipe': recipe}
        value = {'schema_version': SCHEMA_VERSION, 'status': 'ok', 'versions': versions}
        _write_json(result, value)
        for label in LABELS:
            _github_output(label + '_key', versions[label]['key'])
        return value
    except Exception as error:
        _write_json(result, {'schema_version': SCHEMA_VERSION, 'status': 'unavailable',
                            'reason': f'{type(error).__name__}: {error}'})
        raise


def _current_plan(workspace, label):
    value = json.loads((workspace/'ci-results/cache-plan.json').read_text(encoding='utf-8'))
    if value.get('schema_version') != SCHEMA_VERSION or value.get('status') != 'ok':
        raise ValueError('Cache plan is unavailable')
    entry = value['versions'][label]
    recipe = make_recipe(workspace, label)
    if recipe != entry['recipe'] or recipe_key(recipe) != entry['key']:
        raise ValueError('Current build identity does not match the planned recipe')
    return entry


def _enabled():
    return (os.environ.get('BUILD_CACHE_ENABLED') == '1'
            and os.environ.get('GITHUB_REPOSITORY') == TRUSTED_REPOSITORY)


def _trusted_save():
    return (_enabled() and os.environ.get('BUILD_CACHE_ALLOW_SAVE') == '1'
            and os.environ.get('GITHUB_EVENT_NAME') == 'workflow_dispatch'
            and os.environ.get('BUILD_CACHE_DEFAULT_BRANCH') == TRUSTED_DEFAULT_BRANCH
            and os.environ.get('GITHUB_REF') == 'refs/heads/' + TRUSTED_DEFAULT_BRANCH)


def _origin():
    value = {'repository': os.environ.get('GITHUB_REPOSITORY'),
             'event': os.environ.get('GITHUB_EVENT_NAME'), 'ref': os.environ.get('GITHUB_REF'),
             'workflow_sha': os.environ.get('GITHUB_SHA'), 'run_id': os.environ.get('GITHUB_RUN_ID'),
             'run_attempt': os.environ.get('GITHUB_RUN_ATTEMPT')}
    _validate_origin(value)
    return value


def _validate_origin(value):
    if (value.get('repository') != TRUSTED_REPOSITORY or value.get('event') != 'workflow_dispatch'
            or value.get('ref') != 'refs/heads/' + TRUSTED_DEFAULT_BRANCH
            or not re.fullmatch('[0-9a-f]{40}', value.get('workflow_sha', ''))
            or not re.fullmatch('[1-9][0-9]*', value.get('run_id', ''))
            or not re.fullmatch('[1-9][0-9]*', value.get('run_attempt', ''))):
        raise ValueError('Bundle origin is not a successful trusted workflow source')


def _diagnostic(workspace, label, **values):
    path = workspace/'ci-results'/('cache-' + label + '.json')
    try:
        old = json.loads(path.read_text(encoding='utf-8')) if path.is_file() else {}
    except (ValueError, OSError):
        old = {}
    if not isinstance(old, dict):
        old = {}
    old.update(schema_version=SCHEMA_VERSION, label=label, **values)
    _write_json(path, old)


def _regression(log, commands):
    actual_commands = [json.loads(line[8:]) for line in log.splitlines()
                       if line.startswith('COMMAND ')]
    if actual_commands != commands:
        raise ValueError('Build log command sequence differs from the recipe')
    summaries = []
    for line in log.splitlines():
        if not line.startswith('test result:'):
            continue
        match = re.match(r'^test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; '
                         r'(\d+) measured; (\d+) filtered out;', line)
        if not match:
            raise ValueError('Build log contains a failed or unknown regression summary')
        values = [int(v) for v in match.groups()]
        if any(values[1:]):
            raise ValueError('Cached regressions must have no failures or skipped tests')
        summaries.append(values[0])
    if not summaries or not sum(summaries):
        raise ValueError('Build log has no executed passing regressions')
    return {'test_binaries': len(summaries), 'passed': sum(summaries),
            'failed': 0, 'ignored': 0, 'measured': 0, 'filtered_out': 0,
            'passed_by_binary': summaries}


def _build_receipt(receipt, recipe):
    if (receipt.get('schema_version') != SCHEMA_VERSION or receipt.get('status') != 'success'
            or receipt.get('label') != recipe['label'] or receipt.get('suite') != recipe['suite']
            or receipt.get('selection') != recipe['selection'] or receipt.get('reused') is not False
            or receipt.get('commands') != [{'argv': argv, 'returncode': 0}
                                           for argv in recipe['commands']]
            or receipt.get('log') != recipe['label'] + '-build.log'
            or receipt.get('feature_evidence') != recipe['label'] + '-features.json'):
        raise ValueError('Missing or inconsistent successful build receipt')


def _fingerprint_mapping(evidence, recipe):
    spec = recipe['fingerprint_spec']
    if (evidence.get('expected_by_package') != spec['expected']
            or evidence.get('expected_active') != spec['hurt_active']
            or evidence.get('feature') != 'experiment-hurt-readers'):
        raise ValueError('Feature evidence does not match this build recipe')
    mapping = {}
    seen = {package: set() for package in spec['packages']}
    items = evidence.get('fingerprints')
    if not isinstance(items, list) or not items:
        raise ValueError('Missing compiled feature fingerprints')
    for item in items:
        package, kind = item['package'], item['kind']
        if package not in seen or kind not in spec['packages'][package]:
            raise ValueError('Unexpected compiled package or kind')
        target = _relative(item['target_path'])
        if (len(target.parts) != 4 or target.parts[:2] != ('release', '.fingerprint')
                or target.parts[3] != kind
                or not re.fullmatch(re.escape(package) + r'-[A-Za-z0-9_-]+', target.parts[2])):
            raise ValueError('Invalid fingerprint target path')
        artifact = 'fingerprints/' + '/'.join(target.parts[2:])
        if artifact in mapping:
            raise ValueError('Duplicate fingerprint evidence')
        expected_artifact = 'fingerprints/' + recipe['label'] + '/' + '/'.join(target.parts[2:])
        if item['artifact_path'] != expected_artifact:
            raise ValueError('Fingerprint evidence points outside its label')
        features = item['features']
        if not isinstance(features, list) or any(not isinstance(v, str) for v in features):
            raise ValueError('Invalid feature list')
        if len(features) != len(set(features)):
            raise ValueError('Duplicate feature names')
        for feature, active in spec['expected'][package].items():
            if (feature in features) != active:
                raise ValueError('Actual compiled feature activation differs from recipe')
        import ci
        ci.validate_strict_feature_closure(package, features, spec['expected'][package])
        mapping[artifact] = {'kind': 'fingerprint', 'target_path': target.as_posix()}
        seen[package].add(kind)
    if any(seen[p] != set(spec['packages'][p]) for p in seen):
        raise ValueError('Missing compiled package/kind evidence')
    return mapping


def _benchmark_target():
    return 'release/examples/ci_bench' + ('.exe' if os.name == 'nt' else '')


def _mapping(evidence, recipe):
    return {'benchmark/' + Path(_benchmark_target()).name:
                {'kind': 'benchmark', 'target_path': _benchmark_target()},
            'logs/build.log': {'kind': 'log'},
            'evidence/features.json': {'kind': 'feature_evidence'},
            **_fingerprint_mapping(evidence, recipe)}


def _validate_bundle(bundle, entry, label):
    paths = _tree_files(bundle)
    if 'receipt.json' not in paths:
        raise ValueError('Bundle receipt is missing')
    receipt = json.loads(paths['receipt.json'].read_text(encoding='utf-8'))
    recipe = entry['recipe']
    if (receipt.get('schema_version') != SCHEMA_VERSION or receipt.get('status') != 'success'
            or receipt.get('label') != label or receipt.get('key') != entry['key']
            or receipt.get('recipe') != recipe or recipe_key(receipt.get('recipe')) != entry['key']):
        raise ValueError('Bundle recipe or success identity differs from plan')
    _validate_origin(receipt['origin'])
    files = receipt['files']
    if not isinstance(files, dict) or set(paths) != set(files) | {'receipt.json'}:
        raise ValueError('Bundle does not have the exact recorded path set')
    expected_directories = {parent.as_posix() for name in paths
                            for parent in PurePosixPath(name).parents if parent.as_posix() != '.'}
    actual_directories = {path.relative_to(bundle).as_posix() for path in bundle.rglob('*')
                          if path.is_dir()}
    if actual_directories != expected_directories:
        raise ValueError('Bundle contains unexpected empty artifact directories')
    for name, metadata in files.items():
        _relative(name)
        if metadata != {'sha256': digest(paths[name]), 'size': paths[name].stat().st_size}:
            raise ValueError('Bundle file digest or size mismatch: ' + name)
    evidence = json.loads(paths['evidence/features.json'].read_text(encoding='utf-8'))
    mapping = _mapping(evidence, recipe)
    if receipt.get('artifact_mapping') != mapping or set(files) != set(mapping):
        raise ValueError('Unexpected artifact mapping or exact path set')
    benchmark = paths['benchmark/' + Path(_benchmark_target()).name]
    if os.name == 'posix' and not benchmark.stat().st_mode & stat.S_IXUSR:
        raise ValueError('Cached benchmark is not executable by its owner')
    _build_receipt(receipt['build_receipt'], recipe)
    regression = _regression(paths['logs/build.log'].read_text(encoding='utf-8'), recipe['commands'])
    if receipt.get('regression') != regression:
        raise ValueError('Regression receipt count differs from the preserved log')
    for item in evidence['fingerprints']:
        name = 'fingerprints/' + '/'.join(PurePosixPath(item['target_path']).parts[2:])
        if digest(paths[name]) != item['sha256']:
            raise ValueError('Compiler fingerprint SHA differs from feature evidence')
        raw = json.loads(paths[name].read_text(encoding='utf-8'))
        features = raw.get('features')
        if isinstance(features, str):
            features = json.loads(features)
        if features != item['features']:
            raise ValueError('Raw Cargo fingerprint features differ from evidence')
    return receipt, paths


def _cache_root(workspace):
    root = workspace/'verified-build-cache'
    if root.exists() and (root.is_symlink() or root.resolve() != root.absolute()):
        raise ValueError('Cache root resolves outside its named workspace directory')
    root.mkdir(exist_ok=True)
    return root


def _quarantine(workspace, path, label):
    # Rename a verified in-workspace directory; never enumerate paths for deletion.
    expected_parent = workspace/'verified-build-cache'
    if path.parent != expected_parent or path.resolve().parent != expected_parent.resolve():
        raise ValueError('Refusing to move an out-of-scope cache directory')
    if path.is_symlink() or (hasattr(path, 'is_junction') and path.is_junction()):
        raise ValueError('Refusing to move a linked cache root')
    # Rejected material stays outside the Actions artifact upload tree, which must not
    # traverse an invalid bundle's links. Only its safe relative location is in ci-results.
    destination = workspace/'cache-quarantine'/(label + '-' + uuid.uuid4().hex)
    destination.parent.mkdir(parents=True, exist_ok=True)
    path.rename(destination)
    return str(destination.relative_to(workspace))


def restore(workspace, label):
    _label(label)
    workspace = Path(workspace).resolve()
    if not _enabled():
        _diagnostic(workspace, label, status='disabled', reused=False, sealed=False)
        return False
    try:
        import ci
        request_path = workspace/'ci-results/request.json'
        if request_path.is_file() and json.loads(request_path.read_text(encoding='utf-8')).get('candidate_feature') in ci.STRICT_MODES:
            _diagnostic(workspace, label, status='fresh-regressions-required', reused=False, sealed=False,
                        reason='Experimental comparisons require both complete regression arms fresh in this run')
            return False
        entry = _current_plan(workspace, label)
        prefix = label.upper()
        if (os.environ.get(prefix + '_CACHE_HIT') != 'true'
                or os.environ.get(prefix + '_CACHE_MATCHED_KEY') != entry['key']):
            _diagnostic(workspace, label, status='miss', reused=False, sealed=False,
                        key=entry['key'], reason='No exact primary-key cache hit')
            return False
        target = workspace/('target-' + label)
        if target.exists():
            raise ValueError('Cache restore requires a fresh target directory')
        bundle = _cache_root(workspace)/label
        receipt, paths = _validate_bundle(bundle, entry, label)
        # Complete all validation before making the target visible. Staging is kept on errors.
        staging = workspace/('cache-restore-stage-' + label + '-' + uuid.uuid4().hex)
        staging.mkdir()
        for name, item in receipt['artifact_mapping'].items():
            if 'target_path' in item:
                destination = staging/item['target_path']
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(paths[name], destination)
                if {'sha256': digest(destination), 'size': destination.stat().st_size} != receipt['files'][name]:
                    raise ValueError('Artifact changed while staging the cache restore')
        history = workspace/'ci-results/cached'/label
        history.mkdir(parents=True, exist_ok=False)
        for name in ('logs/build.log', 'evidence/features.json', 'receipt.json'):
            destination = history/name
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(paths[name], destination)
        log = workspace/'ci-results'/(label + '-build.log')
        log.write_text('REUSED_VERIFIED_BUILD '+json.dumps(receipt['origin'], sort_keys=True)+'\n'
                       'No Cargo build or regression suite was run for this version in this run.\n',
                       encoding='utf-8')
        _diagnostic(workspace, label, status='hit', reused=True, sealed=False, key=entry['key'],
                    reused_from_run=receipt['origin'], regression=receipt['regression'],
                    cached_receipt=str((history/'receipt.json').relative_to(workspace)))
        # The last fallible installation step is the commit point: a False return must never
        # leave a visible target that would interfere with the cold-build freshness guard.
        staging.rename(target)
        return True
    except Exception as error:
        _diagnostic(workspace, label, status='rejected', reused=False, sealed=False,
                    reason=f'{type(error).__name__}: {error}')
        return False


def seal(workspace, label, feature_evidence):
    _label(label)
    workspace = Path(workspace).resolve()
    _github_output(label + '_cache_sealed', 'false')
    if not _trusted_save():
        _diagnostic(workspace, label, sealed=False, save_reason='Trusted default-branch save is disabled')
        return False
    try:
        entry = _current_plan(workspace, label)
        recipe = entry['recipe']
        result = workspace/'ci-results'
        build_receipt = json.loads((result/(label + '-build-receipt.json')).read_text(encoding='utf-8'))
        _build_receipt(build_receipt, recipe)
        regression = _regression((result/(label + '-build.log')).read_text(encoding='utf-8'),
                                 recipe['commands'])
        evidence_path = result/(label + '-features.json')
        if json.loads(evidence_path.read_text(encoding='utf-8')) != feature_evidence:
            raise ValueError('Feature evidence changed before sealing')
        mapping = _mapping(feature_evidence, recipe)
        cache_root = _cache_root(workspace)
        staging = cache_root/(label + '.staging-' + uuid.uuid4().hex)
        staging.mkdir()
        target = workspace/('target-' + label)
        files = {}
        for name, item in mapping.items():
            if item['kind'] in ('benchmark', 'fingerprint'):
                source = target/item['target_path']
            elif item['kind'] == 'log':
                source = result/(label + '-build.log')
            else:
                source = evidence_path
            if source.is_symlink() or not source.is_file():
                raise ValueError('Cannot seal a missing or linked build artifact')
            destination = staging/name
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, destination)
            files[name] = {'sha256': digest(destination), 'size': destination.stat().st_size}
        receipt = {'schema_version': SCHEMA_VERSION, 'status': 'success', 'label': label,
                   'key': entry['key'], 'recipe': recipe, 'origin': _origin(),
                   'regression': regression, 'build_receipt': build_receipt,
                   'artifact_mapping': mapping, 'files': files}
        _write_json(staging/'receipt.json', receipt)
        _validate_bundle(staging, entry, label)
        # Recheck source, compiler and driver identity after copying the success proof.
        if _current_plan(workspace, label) != entry:
            raise ValueError('Build identity changed while sealing')
        destination = cache_root/label
        quarantine = None
        if destination.exists():
            quarantine = _quarantine(workspace, destination, label)
        staging.rename(destination)
        _diagnostic(workspace, label, sealed=True, save_reason='Successful cold build sealed',
                    key=entry['key'], quarantine=quarantine)
        _github_output(label + '_cache_sealed', 'true')
        return True
    except Exception as error:
        _diagnostic(workspace, label, sealed=False,
                    save_reason=f'{type(error).__name__}: {error}')
        return False


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('stage', choices=('plan',))
    parser.add_argument('--workspace', required=True, type=Path)
    args = parser.parse_args()
    plan(args.workspace)


if __name__ == '__main__':
    main()
