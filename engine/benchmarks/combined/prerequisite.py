"""Require the exact authorized combined speed run to finish successfully before testing."""
import json
import os
from pathlib import Path
import urllib.request

REPOSITORY = 'andrew0416/pokemon-ai-lab'
RUN_ID = 36665429800
HEAD_SHA = '25ee2e42d7b17efc0d0519e0bb936fcbb38d776c'


def validate(run):
    if (run.get('id') != RUN_ID or run.get('head_sha') != HEAD_SHA
            or run.get('repository', {}).get('full_name') != REPOSITORY
            or run.get('event') != 'workflow_dispatch'
            or run.get('status') != 'completed' or run.get('conclusion') != 'success'):
        raise ValueError('The exact prerequisite combined run has not completed successfully')
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
    path = Path(os.environ.get('GITHUB_WORKSPACE', '.')) / 'combined-prerequisite.json'
    with path.open('x', encoding='utf-8') as stream:
        json.dump(receipt, stream, indent=2)
        stream.write('\n')
    print(json.dumps(receipt))


if __name__ == '__main__':
    main()
