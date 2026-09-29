"""Filter an action-restored target for external-dependency compilation reuse.

This is a compilation aid, not regression proof. Every admitted target still runs
all Cargo commands and compiler-feature checks. Rejected targets are quarantined
outside uploaded results; no recursive deletion is performed.
"""
import json
import os
from pathlib import Path
import re
import stat
import uuid


def _linked(path):
    info = path.lstat()
    return (path.is_symlink() or stat.S_ISLNK(info.st_mode)
            or bool(getattr(info, 'st_file_attributes', 0)
                    & getattr(stat, 'FILE_ATTRIBUTE_REPARSE_POINT', 0)))


def _inspect(target):
    if _linked(target) or not target.is_dir():
        raise ValueError('Dependency target must be a regular directory, without links')
    count = 0
    for folder, directories, files in os.walk(target, followlinks=False):
        for name in directories + files:
            path = Path(folder)/name
            relative = path.relative_to(target).as_posix()
            if _linked(path):
                raise ValueError(f'Dependency target contains a link: {relative}')
            mode = path.lstat().st_mode
            if not (stat.S_ISREG(mode) or stat.S_ISDIR(mode)):
                raise ValueError(f'Dependency target contains a special file: {relative}')
            if re.match(r'^(?:lib)?lab[-_]', name):
                raise ValueError(f'Dependency target contains a workspace artifact: {relative}')
            if 'examples' in path.relative_to(target).parts[:-1]:
                raise ValueError(f'Dependency target contains an example artifact: {relative}')
            if (stat.S_ISREG(mode) and path.parent == target/'release/deps'
                    and path.suffix.lower() in ('', '.exe')):
                raise ValueError(f'Dependency target contains a test executable: {relative}')
            if stat.S_ISREG(mode):
                count += 1
    return count


def prepare_target(workspace, label):
    """Return True for a safe seeded target; False for absent/rejected targets.

    An unexplained existing target (no completed restore-action marker) remains
    an error. A marked but contaminated target is moved as one directory entry,
    never traversed for deletion, and the caller builds into its now-absent path.
    The marker does not certify a download; Cargo checks all admitted inputs.
    """
    if label not in ('baseline', 'candidate'):
        raise ValueError('Invalid dependency-cache build label')
    workspace = Path(workspace).resolve(strict=True)
    target = workspace/('target-' + label)
    result = workspace/'ci-results'
    receipt_path = result/('dependency-' + label + '.json')
    receipt = {'schema_version': 1, 'label': label, 'status': 'absent',
               'target': str(target), 'reuses_regression_proof': False}

    def save():
        receipt_path.write_text(json.dumps(receipt, indent=2)+'\n', encoding='utf-8')

    if not os.path.lexists(target):
        save()
        return False
    if os.environ.get(label.upper() + '_DEPENDENCY_CACHE_READY') != '1':
        receipt.update(status='unmarked', reason='No completed dependency restore-action marker')
        save()
        raise ValueError(f'{label}: target directory must be new unless dependency cache is marked ready')
    try:
        count = _inspect(target)
    except (OSError, ValueError) as error:
        quarantine = workspace/'dependency-cache-quarantine'
        if os.path.lexists(quarantine) and (_linked(quarantine) or not quarantine.is_dir()):
            raise ValueError('Dependency quarantine must be a regular directory') from error
        quarantine.mkdir(exist_ok=True)
        destination = quarantine/(label + '-' + uuid.uuid4().hex)
        # Both lexical entries are direct children of known workspace directories.
        # Do not resolve a rejected target link and accidentally move its referent.
        if target.parent != workspace or destination.parent.resolve() != quarantine:
            raise ValueError('Unsafe dependency quarantine destination') from error
        receipt.update(status='rejected', reason=f'{type(error).__name__}: {error}',
                       quarantine=str(destination))
        save()
        target.rename(destination)
        return False
    receipt.update(status='accepted', regular_files=count,
                   reason='Passed external-dependency shape checks; full Cargo validation and regression gate remain required')
    save()
    return True
