"""Require the exact authorized P9 speed run to finish successfully before testing."""
import json
import os
from pathlib import Path
import urllib.request

REPOSITORY = 'andrew0416/pokemon-ai-lab'
RUN_ID = 36660421636
HEAD_SHA = '62ac54c2c9681fa3d598128b0ed72c6f4a2b3db9'


def validate(run):
    if (run.get('id') != RUN_ID or run.get('head_sha') != HEAD_SHA
            or run.get('repository', {}).get('full_name') != REPOSITORY
            or run.get('event') != 'workflow_dispatch'
            or run.get('status') != 'completed' or run.get('conclusion') != 'success'):
        raise ValueError('The exact prerequisite P9 run has not completed successfully')
    return {key: run[key] for key in ('id', 'head_sha', 'status', 'conclusion', 'html_url')}


def main():
    token = os.environ.get('GITHUB_TOKEN', '')
    if not token:
        raise ValueError('Read-only GitHub token is required')
    request = urllib.request.Request(
        f'https://api.github.com/repos/{REPOSITORY}/actions/runs/{RUN_ID}',
        headers={'Authorization': 'Bearer ' + token, 'Accept': 'application/vnd.github+json',
                 'X-GitHub-Api-Version': '2022-11-28', 'User-Agent': 'combined-engine-prerequisite'})
    with urllib.request.urlopen(request, timeout=45) as response:
        receipt = validate(json.load(response))
    path = Path(os.environ.get('GITHUB_WORKSPACE', '.')) / 'p9-prerequisite.json'
    with path.open('x', encoding='utf-8') as stream:
        json.dump(receipt, stream, indent=2)
        stream.write('\n')
    print(json.dumps(receipt))


if __name__ == '__main__':
    main()
