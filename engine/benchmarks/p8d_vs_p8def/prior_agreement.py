"""Reuse pinned broad agreement through old4; this is not a new D-vs-DEF run."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import sys
import urllib.request

HERE = Path(__file__).resolve().parent
REPO = 'andrew0416/pokemon-ai-lab'
REPO_ID = 1390971025
SOURCE = '048e4748e8854dac5eb511a358cc2f8c38c9dc29'
MODE = 'p8d-vs-p8def'
EXPECTED_RUNS = {'d': 36681590730, 'def': 36698588329}
EXPECTED_HEADS = {'d': '6dcc56a6398702c2281019dd00fe5ee4eb7a2019',
                  'def': '7bfb3380cc12439d368a4a288d00fe1269c00455'}
SUMMARY_SHA256 = {'d': '362c7c259a5b629019a58b6b0444e1da76865b26a8e5c9178f5566621b0e2e09',
                  'def': '87ee0c513eb2dc64c98ab5a8bdc322aa3337815999a2784e15ab0594fb88fa13'}
ARTIFACT_IDS = {'d': 11082333020, 'def': 11089905329}
ARTIFACT_DIGESTS = {'d': 'sha256:499674340b9cb3737715bc96e2d796bde1e439a9ca936cd084d2fad8369a0c89',
                    'def': 'sha256:ee2696a00f9dfe0d4c3f387b08140e7ede9426c530378eb74aaa50d31ecfb6c8'}
PINS_SHA256 = '2c8e1f71438acd0cef01073a3f3e9b438c89f7b4cea073c9a73d800ee5d78b5f'
JOBS = {
    'd': {109778010837: 'agreement_plan', 109782137898: 'agreement_compare',
          109778067574: 'p8d', 109778067662: 'p8e', 109778067685: 'p8f', 109778067708: 'base',
          109782315567: 'speed replay-action-keys'},
    'def': {109832399450: 'agreement_plan', 109832474021: 'base', 109832474053: 'p8def_combined',
            109837229144: 'agreement_compare', 109837358744: 'speed p8def-combined'},
}
OLD4_RUNTIME = {'lab-engine/experiment-hurt-readers', 'lab-engine/experiment-compact-volatiles',
                'lab-search/experiment-prepared-turn', 'lab-search/experiment-leaf-ending-states'}
RUNTIME_FEATURES = {
    'baseline': OLD4_RUNTIME | {'lab-engine/experiment-replay-action-keys'},
    'candidate': OLD4_RUNTIME | {'lab-engine/experiment-replay-action-keys',
                                'lab-engine/experiment-slot-diff', 'lab-engine/experiment-stats-off-cost'},
}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def strict_json(raw):
    def pairs(items):
        out = {}
        for key, value in items:
            require(key not in out, 'Duplicate JSON key: ' + key)
            out[key] = value
        return out
    return json.loads(raw, object_pairs_hook=pairs,
                      parse_constant=lambda value: (_ for _ in ()).throw(ValueError('Non-finite JSON: ' + value)))


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def load_pins():
    raw = (HERE / 'controller-pins.json').read_bytes()
    require(sha(raw) == PINS_SHA256, 'Controller pin manifest changed')
    document = strict_json(raw)
    require(document['source_sha'] == SOURCE and set(document['controllers']) == {'d', 'def'},
            'Wrong pinned source/controllers')
    for label in EXPECTED_RUNS:
        require(document['controllers'][label]['head_sha'] == EXPECTED_HEADS[label], 'Wrong controller head')
    for path in document['shared_frozen_paths']:
        require(document['controllers']['d']['files'][path] == document['controllers']['def']['files'][path],
                'Prior corpus/probe/contract differs: ' + path)
    return document


def load_module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def contracts():
    directory = HERE.parent / 'agreement'
    if str(directory) not in sys.path:
        sys.path.insert(0, str(directory))
    import p8def_contract
    import p8def_combined_contract
    return p8def_contract, p8def_combined_contract


def validate_source(benchmarks=None, feature_reader=None):
    benchmarks = Path(benchmarks) if benchmarks is not None else HERE.parent
    pins = load_pins()
    expected = pins['controllers']['def']['files']
    source_root = benchmarks.parent.parent
    for relative, hashes in expected.items():
        path = source_root / relative
        require(path.is_file() and sha(path.read_bytes()) == hashes['sha256'],
                'Frozen controller file changed: ' + relative)
    independent, combined = contracts()
    old = pins['controllers']['d']['variants']
    new = pins['controllers']['def']['variants']
    independent.validate_variants(old)
    combined.validate_variants(new)
    require(all(v['sha'] == SOURCE for document in (old, new) for v in document['variants']),
            'Source changed; fresh broad accuracy required')
    require(old['variants'][0] == new['variants'][0], 'No identical old4 baseline')
    require(strict_json((benchmarks / 'agreement/variants.json').read_bytes()) == new,
            'Current source/variant contract changed')
    if feature_reader is None:
        feature_reader = load_module('direct_paired_ci', benchmarks / 'paired/ci.py').feature_args
    actual = {}
    for label, wanted in RUNTIME_FEATURES.items():
        argv = feature_reader(MODE, label)
        require(isinstance(argv, list) and len(argv) == 2 and argv[0] == '--features'
                and isinstance(argv[1], str), 'Wrong paired feature CLI')
        features = argv[1].split(',')
        require(len(features) == len(wanted) and set(features) == wanted,
                'Direct timing features changed: ' + label)
        actual[label] = sorted(features)
    return {'source_sha': SOURCE, 'controller_pins_sha256': PINS_SHA256,
            'shared_frozen_file_count': len(pins['shared_frozen_paths']),
            'agreement_controller_heads': EXPECTED_HEADS, 'paired_mode': MODE,
            'timing_features': actual, 'p11_enabled': False}


def validate_remote(label, run, listing):
    require(label in EXPECTED_RUNS, 'Unknown evidence arm')
    run_id, head = EXPECTED_RUNS[label], EXPECTED_HEADS[label]
    require(type(run.get('id')) is int and run['id'] == run_id and run.get('head_sha') == head
            and run.get('repository', {}).get('full_name') == REPO
            and run.get('head_repository', {}).get('full_name') == REPO
            and run.get('status') == 'completed' and run.get('event') == 'workflow_dispatch'
            and type(run.get('run_attempt')) is int and run['run_attempt'] == 1
            and run.get('path') == '.github/workflows/engine-benchmark.yml'
            and run.get('conclusion') == ('failure' if label == 'd' else 'success'),
            'Wrong prior run identity/status: ' + label)
    rows = listing.get('jobs')
    require(isinstance(rows, list) and type(listing.get('total_count')) is int
            and listing['total_count'] == len(rows) and all(isinstance(row, dict) for row in rows),
            'Incomplete jobs listing')
    ids = [row.get('id') for row in rows]
    require(all(type(value) is int for value in ids) and len(ids) == len(set(ids)), 'Duplicate/invalid job identity')
    selected = {row['id']: row for row in rows if row['id'] in JOBS[label]}
    require(set(selected) == set(JOBS[label]), 'Missing required agreement/speed job')
    for job_id, name in JOBS[label].items():
        row = selected[job_id]
        name_matches = (row.get('name') == name if name.startswith(('agreement_', 'speed '))
                        else row.get('name', '').startswith(f'agreement_evaluate ({name}, {SOURCE},'))
        require(name_matches and row.get('run_id') == run_id and row.get('head_sha') == head
                and type(row.get('run_attempt')) is int and row['run_attempt'] == 1
                and row.get('status') == 'completed' and row.get('conclusion') == 'success',
                'Required prior job failed/changed: ' + name)
    return {'run_id': run_id, 'head_sha': head, 'run_attempt': 1,
            'overall_conclusion': run['conclusion'], 'required_successful_job_ids': sorted(selected)}


def validate_artifact(label, artifact):
    owner = artifact.get('workflow_run', {})
    require(artifact.get('id') == ARTIFACT_IDS[label] and artifact.get('name') == 'agreement-summary'
            and artifact.get('expired') is False and artifact.get('digest') == ARTIFACT_DIGESTS[label]
            and owner.get('id') == EXPECTED_RUNS[label] and owner.get('head_sha') == EXPECTED_HEADS[label]
            and owner.get('repository_id') == REPO_ID and owner.get('head_repository_id') == REPO_ID,
            'Wrong/expired summary artifact: ' + label)


def validate_controller_tree(label, commit, tree):
    require(commit.get('sha') == EXPECTED_HEADS[label] and isinstance(commit.get('tree'), dict)
            and isinstance(tree.get('sha'), str) and re.fullmatch('[0-9a-f]{40}', tree['sha'])
            and tree.get('sha') == commit['tree'].get('sha') and tree.get('truncated') is False,
            'Wrong or truncated immutable controller tree')
    rows = [row for row in tree.get('tree', []) if row.get('path') == 'engine/benchmarks/agreement']
    require(len(rows) == 1 and rows[0].get('type') == 'tree'
            and rows[0].get('sha') == load_pins()['controllers'][label]['agreement_tree'],
            'Prior corpus/probe/variant/contract tree changed')


def validate_remote_document(document):
    require(type(document.get('schema')) is int and document['schema'] == 1
            and set(document.get('evidence', {})) == {'d', 'def'},
            'Missing both remote evidence records')
    receipts = {}
    for label, value in document['evidence'].items():
        receipts[label] = validate_remote(label, value['run'], value['jobs'])
        validate_artifact(label, value['artifact'])
        validate_controller_tree(label, value['commit'], value['tree'])
    return receipts


def validate_summary_data(label, summary):
    require(label in EXPECTED_RUNS, 'Unknown summary arm')
    independent, combined = contracts()
    (independent if label == 'd' else combined).validate_summary(summary)
    key = 'p8d' if label == 'd' else 'p8def_combined'
    row = summary['comparisons'][key]
    require(row['equal_success'] == 6184 and all(row[name] == 0 for name in ('equal_error', 'different', 'uncompared')),
            'Relevant comparison is incomplete')
    require(row['comparison_contract'] == ('exact-v1' if label == 'd' else independent.STATE_CONTRACT),
            'Relevant equality contract changed')
    require(row['raw_turn_different_ids'] == [] if label == 'd' else len(row['raw_turn_different_ids']) == 1505,
            'Raw instruction-difference ledger changed')


def validate_summaries(d_path, def_path):
    source = validate_source()
    summaries = {}
    for label, path in (('d', d_path), ('def', def_path)):
        raw = Path(path).read_bytes()
        require(sha(raw) == SUMMARY_SHA256[label], 'Prior summary hash mismatch: ' + label)
        summaries[label] = strict_json(raw)
        validate_summary_data(label, summaries[label])
    old_ids = summaries['d']['comparisons']['p8e']['raw_turn_different_ids']
    new_ids = summaries['def']['comparisons']['p8def_combined']['raw_turn_different_ids']
    require(len(old_ids) == 1505 and old_ids == new_ids, 'P8e raw difference identities changed')
    # Keep the original 1,505 raw SHA/byte rows visible, not just an accepted count.
    return {'schema': 1, 'accepted': True, **source, 'runs': EXPECTED_RUNS,
            'summary_sha256': SUMMARY_SHA256, 'summary_artifact_ids': ARTIFACT_IDS,
            'cases_per_variant': 6184, 'comparison_route': 'old4+D == old4; old4+DEF state-equivalent to old4',
            'proof_scope': 'Reused frozen broad agreement through the shared old4 baseline; not fresh direct D-vs-DEF execution.',
            'fresh_direct_broad_execution': False, 'fresh_regressions_observers_probes_and_timing_still_required': True,
            'd_contract': 'exact-v1', 'def_contract': 'state-equivalent-slot-diff-v1',
            'raw_turn_different_ids': new_ids,
            'raw_turn_differences': summaries['def']['comparisons']['p8def_combined']['raw_turn_differences']}


def api(suffix):
    token = os.environ.get('GITHUB_TOKEN')
    require(bool(token), 'Read-only Actions token required')
    request = urllib.request.Request(f'https://api.github.com/repos/{REPO}/{suffix}', headers={
        'Authorization': 'Bearer ' + token, 'Accept': 'application/vnd.github+json',
        'X-GitHub-Api-Version': '2022-11-28', 'User-Agent': 'p8d-vs-p8def-prior-agreement'})
    with urllib.request.urlopen(request, timeout=45) as response:
        return strict_json(response.read())


def write_new(path, value):
    with Path(path).open('x', encoding='utf8') as stream:
        json.dump(value, stream, indent=2)
        stream.write('\n')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--remote', action='store_true')
    parser.add_argument('--summaries', nargs=2, type=Path, metavar=('D_SUMMARY', 'DEF_SUMMARY'))
    args = parser.parse_args()
    require(args.remote or args.summaries, 'No verification requested')
    validate_source()
    if args.remote:
        document = {'schema': 1, 'evidence': {}}
        for label, run_id in EXPECTED_RUNS.items():
            commit = api(f'git/commits/{EXPECTED_HEADS[label]}')
            document['evidence'][label] = {
                'run': api(f'actions/runs/{run_id}'),
                'jobs': api(f'actions/runs/{run_id}/jobs?filter=all&per_page=100'),
                'artifact': api(f'actions/artifacts/{ARTIFACT_IDS[label]}'), 'commit': commit,
                'tree': api(f'git/trees/{commit["tree"]["sha"]}?recursive=1')}
        receipts = validate_remote_document(document)
        write_new('prior-agreement-remote.json', document)
        print(json.dumps({'remote_verified': receipts}))
    if args.summaries:
        remote_path = Path('prior-agreement-remote.json')
        require(remote_path.is_file(), 'Verify --remote before accepting downloaded summaries')
        remote = validate_remote_document(strict_json(remote_path.read_bytes()))
        receipt = validate_summaries(*args.summaries)
        receipt['remote_verified'] = remote
        receipt['remote_record_sha256'] = sha(remote_path.read_bytes())
        write_new('prior-agreement-receipt.json', receipt)
        print('Pinned 6184-case D and DEF agreement accepted through old4; fresh direct broad execution not claimed')


if __name__ == '__main__':
    main()
