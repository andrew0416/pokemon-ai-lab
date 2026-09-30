"""Require the exact successful agreement phase before a source-identical P8f retry."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import sys
import urllib.request

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / 'agreement'))
import p8def_contract as contract

RUN = 36681590730
HEAD = '6dcc56a6398702c2281019dd00fe5ee4eb7a2019'
SOURCE = '048e4748e8854dac5eb511a358cc2f8c38c9dc29'
REPO = 'andrew0416/pokemon-ai-lab'
SUMMARY_SHA256 = '362c7c259a5b629019a58b6b0444e1da76865b26a8e5c9178f5566621b0e2e09'
ARTIFACT = 11082333020
JOBS = {109778010837: 'agreement_plan', 109782137898: 'agreement_compare',
        109778067574: 'p8d', 109778067662: 'p8e', 109778067685: 'p8f', 109778067708: 'base'}

def validate_remote(run, listing):
    require = contract.require
    require(run.get('id') == RUN and run.get('head_sha') == HEAD
            and run.get('repository', {}).get('full_name') == REPO
            and run.get('status') == 'completed' and run.get('event') == 'workflow_dispatch'
            and run.get('run_attempt') == 1, 'Wrong prior run identity')
    rows = listing.get('jobs', [])
    require(listing.get('total_count') == len(rows), 'Incomplete job listing')
    selected = [r for r in rows if r.get('id') in JOBS]
    require(len(selected) == len(JOBS) and len({r['id'] for r in selected}) == len(JOBS), 'Missing/duplicate agreement job')
    for row in selected:
        name = JOBS[row['id']]
        expected = row.get('name') == name if name.startswith('agreement_') else row.get('name', '').startswith('agreement_evaluate (' + name + ',')
        require(expected and row.get('run_id') == RUN and row.get('head_sha') == HEAD
                and row.get('status') == 'completed' and row.get('conclusion') == 'success', 'Prior agreement job failed or changed')
    return {'run_id': RUN, 'head_sha': HEAD, 'source_sha': SOURCE,
            'agreement_job_ids': sorted(JOBS), 'summary_artifact_id': ARTIFACT,
            'summary_sha256': SUMMARY_SHA256, 'engine_source_changed': False}

def validate_source():
    document = json.loads((HERE.parent / 'agreement/variants.json').read_text(encoding='utf8'))
    contract.validate_variants(document)
    contract.require(all(v['sha'] == SOURCE for v in document['variants']), 'Source changed; fresh broad accuracy required')

def validate_summary(path):
    raw = Path(path).read_bytes()
    contract.require(hashlib.sha256(raw).hexdigest() == SUMMARY_SHA256, 'Prior summary hash mismatch')
    contract.validate_summary(json.loads(raw))
    validate_source()

def api(suffix):
    token = os.environ.get('GITHUB_TOKEN')
    contract.require(bool(token), 'Read-only Actions token required')
    request = urllib.request.Request(f'https://api.github.com/repos/{REPO}/{suffix}', headers={
        'Authorization': 'Bearer ' + token, 'Accept': 'application/vnd.github+json',
        'X-GitHub-Api-Version': '2022-11-28', 'User-Agent': 'p8f-agreement-retry'})
    with urllib.request.urlopen(request, timeout=45) as response: return json.load(response)

def main():
    parser = argparse.ArgumentParser(); parser.add_argument('--remote', action='store_true'); parser.add_argument('--summary', type=Path)
    args = parser.parse_args(); contract.require(args.remote or args.summary, 'No verification requested')
    validate_source()
    if args.remote:
        receipt = validate_remote(api(f'actions/runs/{RUN}'), api(f'actions/runs/{RUN}/jobs?per_page=100'))
        artifact = api(f'actions/artifacts/{ARTIFACT}')
        contract.require(artifact.get('id') == ARTIFACT and artifact.get('name') == 'agreement-summary'
            and artifact.get('expired') is False and artifact.get('workflow_run', {}).get('id') == RUN
            and artifact.get('workflow_run', {}).get('head_sha') == HEAD, 'Wrong summary artifact')
        with Path('prior-agreement-receipt.json').open('x', encoding='utf8') as stream:
            json.dump(receipt, stream, indent=2); stream.write('\n')
        print(json.dumps(receipt))
    if args.summary:
        validate_summary(args.summary); print('Exact prior 6184-case agreement accepted; engine source unchanged')

if __name__ == '__main__': main()
