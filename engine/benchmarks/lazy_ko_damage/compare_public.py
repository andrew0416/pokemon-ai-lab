"""Compare retained full-State/Suspension distributions, allowing only probability roundoff."""
from pathlib import Path
import argparse
import hashlib
import json
import math

CASES = ('all-ko', 'ko-threshold', 'drain', 'recoil', 'shell-bell', 'innards-out',
         'liquid-ooze', 'life-orb', 'uturn-suspension', 'multihit-survive')
FIELDS = {'schema', 'case', 'rolls', 'input', 'input_hash', 'components', 'distribution', 'samples'}
TOLERANCE = 1e-12


def require(ok, message):
    if not ok:
        raise ValueError(message)


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, 'duplicate JSON key')
        result[key] = value
    return result


def read(path):
    raw = Path(path).read_bytes()
    require(raw.endswith(b'\n'), 'incomplete final record')
    rows = [json.loads(line, object_pairs_hook=unique_object,
                       parse_constant=lambda x: (_ for _ in ()).throw(ValueError('nonfinite JSON')))
            for line in raw.decode('utf-8').splitlines()]
    require([r['case'] for r in rows] == list(CASES), 'unexpected case order, missing or extra case')
    for row in rows:
        require(set(row) == FIELDS and row['schema'] == 1, 'unexpected record schema')
        require(row['rolls'] == ('Full' if row['case'] == 'all-ko' else 'Extremes'), 'wrong rolls')
        require(type(row['components']) is int and row['components'] > 0, 'empty component output')
        require(type(row['input_hash']) is int and isinstance(row['input'], str), 'input identity missing')
        for field in ('distribution', 'samples'):
            dist = row[field]
            require(isinstance(dist, dict) and bool(dist), 'empty distribution')
            for key, p in dist.items():
                require(isinstance(key, str) and '\n' in key and key.startswith('State {'), 'missing full State/Suspension key')
                require(type(p) in (int, float) and math.isfinite(p) and p >= 0, 'invalid probability')
            require(abs(math.fsum(dist.values()) - 1.0) <= TOLERANCE, 'non-unit mass')
    return rows, hashlib.sha256(raw).hexdigest()


def compare_records(off: Path, on: Path) -> dict:
    left, off_sha = read(off)
    right, on_sha = read(on)
    max_error, total_keys, changes = 0.0, 0, []
    for a, b in zip(left, right):
        name = a['case']
        for field in ('schema', 'case', 'rolls', 'input', 'input_hash', 'samples'):
            require(a[field] == b[field], f'{name}: changed {field}')
        require(a['distribution'].keys() == b['distribution'].keys(), f'{name}: changed full State/Suspension support')
        error = max(abs(p - b['distribution'][key]) for key, p in a['distribution'].items())
        require(error <= TOLERANCE, f'{name}: probability mismatch {error}')
        max_error = max(max_error, error)
        total_keys += len(a['distribution'])
        changes.append({'case': name, 'off_components': a['components'], 'on_components': b['components'],
                        'full_state_keys': len(a['distribution']), 'max_abs_probability_error': error})
    return {'passed': True, 'cases': len(left), 'full_state_suspension_keys': total_keys,
            'support_exact': True, 'sample_distributions_exact': True,
            'probability_abs_tolerance': TOLERANCE, 'max_abs_probability_error': max_error,
            'off_sha256': off_sha, 'on_sha256': on_sha, 'component_counts_may_differ': True,
            'case_results': changes, 'scope': 'Bounded synthetic fixtures; no universal correctness proof.'}


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('off', type=Path)
    parser.add_argument('on', type=Path)
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    result = compare_records(args.off, args.on)
    if args.output:
        with args.output.open('x', encoding='utf-8') as stream:
            json.dump(result, stream, indent=2)
            stream.write('\n')
    print(json.dumps(result))
