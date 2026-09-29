"""Archive an existing oracle corpus without regenerating any engine/oracle output.

Only this packaging command runs locally. It reads the original corpus, validates every
gzip/JSON input, then writes a NEW output directory. CI consumes the immutable archive and
manifest. Search selection is a bounded sample, not 256 independent games or a new oracle.
"""
from __future__ import annotations

import argparse
from collections import Counter, defaultdict
import gzip
import hashlib
import io
import json
import math
from pathlib import Path, PurePosixPath
import tarfile


SCHEMA = 1
ARCHIVE = 'corpus.tar.gz'


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def canonical(value) -> bytes:
    return json.dumps(value, sort_keys=True, ensure_ascii=False, separators=(',', ':'),
                      allow_nan=False).encode('utf-8')


def parse(raw: bytes):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError(f'Duplicate JSON key: {key}')
            result[key] = value
        return result
    def invalid(value):
        raise ValueError(f'Non-finite JSON number: {value}')
    return json.loads(raw, object_pairs_hook=unique, parse_constant=invalid)


def source_file(root: Path, name: str) -> Path:
    rel = PurePosixPath(name)
    if not name or '\\' in name or rel.is_absolute() or '..' in rel.parts or ':' in name:
        raise ValueError(f'Unsafe corpus path: {name!r}')
    path = root.joinpath(*rel.parts)
    for part in (path, *path.parents):
        if part == root:
            break
        if part.is_symlink():
            raise ValueError(f'Corpus links are not supported: {path}')
    if not path.is_file() or not path.resolve().is_relative_to(root.resolve()):
        raise ValueError(f'Missing or escaping corpus file: {path}')
    return path


def stage(turn: int) -> str:
    return 'early' if turn <= 5 else 'middle' if turn <= 15 else 'late'


def features(before: dict) -> dict[str, list[str]]:
    """Observed decision-state features; absence here is not an engine coverage claim."""
    found = defaultdict(set)
    field = before.get('field', {})
    for key in ('weather', 'terrain'):
        if field.get(key):
            found[key].add(str(field[key]))
    found['pseudo_weather'].update(field.get('pseudoWeather', {}))
    for side in before.get('sides', []):
        found['request'].add(str(side.get('request', '')))
        found['side_condition'].update(side.get('conditions', {}))
        for slot in side.get('slotConditions', []):
            found['slot_condition'].update(slot)
        for pokemon in side.get('pokemon', []):
            for key in ('species', 'status', 'item'):
                if pokemon.get(key):
                    found[key].add(str(pokemon[key]))
            if pokemon.get('slot') is not None:
                if pokemon.get('ability'):
                    found['active_ability'].add(str(pokemon['ability']))
                found['volatile'].update(pokemon.get('volatiles', {}))
                for stat, value in pokemon.get('boosts', {}).items():
                    if value:
                        found['boost'].add(stat + ('+' if value > 0 else '-'))
    return {key: sorted(values) for key, values in sorted(found.items())}


def coverage(jobs: list[dict]) -> dict:
    result = {'jobs': len(jobs), 'unique_before_states': len({j['before_sha256'] for j in jobs}),
              'games': len({(j['set'], j['game']) for j in jobs}),
              'matchups': len({j['matchup'] for j in jobs})}
    for key in ('set', 'kind', 'policy', 'stage', 'mode', 'turn'):
        result[key] = dict(sorted(Counter(str(j[key]) for j in jobs).items()))
    seen = defaultdict(Counter)
    for job in jobs:
        for key, values in job['before_features'].items():
            seen[key].update(values)
    result['before_features_positions'] = {key: dict(sorted(value.items()))
                                           for key, value in sorted(seen.items())}
    return result


def select_search(jobs: list[dict], size: int) -> tuple[list[dict], dict]:
    """Deduplicate before states, cover games first, then matchup/stage round robin."""
    eligible = [j for j in jobs if j['search_boundary_eligible']]
    unique = {}
    for job in sorted(eligible, key=lambda j: j['id']):
        unique.setdefault(job['before_sha256'], job)
    pool = list(unique.values())
    if len(pool) < size:
        raise ValueError(f'Only {len(pool)} unique eligible before states for search size {size}')
    selected = []
    used = set()
    games = Counter()
    stages = Counter()
    turns = Counter()
    observed_features = set()
    matchups = sorted({j['matchup'] for j in pool})
    # First pass: one unique before state for each represented game, with matchup round robin.
    # Second pass fills the sample while preferring each matchup's least represented stage.
    for game_pass in (True, False):
        while len(selected) < size:
            progress = False
            for matchup in matchups:
                options = [j for j in pool if j['matchup'] == matchup
                           and j['before_sha256'] not in used
                           and (not game_pass or not games[(j['set'], j['game'])])]
                if not options:
                    continue
                def ranking(candidate):
                    values = {(key, value) for key, items in candidate['before_features'].items()
                              for value in items}
                    # Avoid selecting the lexically earliest turn of every game. The
                    # finite counters preserve stage/game balance while broadening
                    # turn numbers and observed features; SHA is a stable final tie break.
                    return (stages[(matchup, candidate['stage'])],
                            games[(candidate['set'], candidate['game'])],
                            turns[candidate['turn']], -len(values - observed_features),
                            digest(candidate['id'].encode('utf-8')))
                job = min(options, key=ranking)
                selected.append(job)
                used.add(job['before_sha256'])
                games[(job['set'], job['game'])] += 1
                stages[(matchup, job['stage'])] += 1
                turns[job['turn']] += 1
                observed_features.update((key, value) for key, items in job['before_features'].items()
                                         for value in items)
                progress = True
                if len(selected) == size:
                    break
            if not progress:
                break
    if len(selected) != size:
        raise ValueError('Search selection did not reach the requested size')
    return selected, {'eligible_entries': len(eligible), 'unique_eligible_before_states': len(pool),
                      'eligible_games_after_dedup': len({(j['set'], j['game']) for j in pool}),
                      'eligible_matchups': len(matchups), 'selected': coverage(selected)}


def make_pack(corpus: Path, out: Path, search_size: int = 256,
              expected_positions: int | None = 1781, expected_reports: int | None = 2052) -> dict:
    corpus = corpus.resolve()
    out = out.resolve()
    if out.exists() or out.is_relative_to(corpus):
        raise ValueError('Output must be a new directory outside the original corpus')
    if search_size <= 0:
        raise ValueError('Search size must be positive')
    index_path = source_file(corpus, 'corpus.json')
    original_index = index_path.read_bytes()
    index = parse(original_index)
    entries = index['positions']
    if len({e['id'] for e in entries}) != len(entries):
        raise ValueError('Duplicate corpus position ID')
    if any(e['build'] not in ('ok', 'excluded') for e in entries):
        raise ValueError('Pending or failed corpus entries cannot be silently omitted')
    included = [e for e in entries if e['build'] == 'ok']
    report_count = sum(len(e['reports']) for e in included)
    if expected_positions is not None and len(included) != expected_positions:
        raise ValueError('Unexpected included position count')
    if expected_reports is not None and report_count != expected_reports:
        raise ValueError('Unexpected report count')
    members = {}
    paths = {}
    def record(name, role, raw_size=None, gz_size=None):
        if name in members:
            raise ValueError(f'Duplicate archive member reference: {name}')
        path = source_file(corpus, name)
        data = path.read_bytes()
        raw = gzip.decompress(data) if name.endswith('.gz') else data
        if raw_size is not None and len(raw) != raw_size:
            raise ValueError(f'Uncompressed size differs from original index: {name}')
        if gz_size is not None and len(data) != gz_size:
            raise ValueError(f'Compressed size differs from original index: {name}')
        members[name] = {'role': role, 'bytes': len(data), 'sha256': digest(data),
                         'uncompressed_bytes': len(raw), 'uncompressed_sha256': digest(raw)}
        paths[name] = path
        return parse(raw)
    record('corpus.json', 'original_index')
    for name in ('sources.json', 'README.md', 'features.json', 'features.md'):
        path = corpus/name
        if path.is_file():
            data = path.read_bytes()
            members[name] = {'role': 'historical_provenance', 'bytes': len(data),
                             'sha256': digest(data), 'uncompressed_bytes': len(data),
                             'uncompressed_sha256': digest(data)}
            paths[name] = source_file(corpus, name)
    turn_jobs, oracle_jobs = [], []
    raw_scenarios = defaultdict(list)
    before_groups = defaultdict(list)
    modes = Counter()
    versions = Counter()
    all_before = set()
    for entry in sorted(included, key=lambda e: e['id']):
        scenario = record(entry['scenario'], 'scenario', entry['scenario_bytes'], entry['scenario_gz_bytes'])
        raw_scenarios[members[entry['scenario']]['uncompressed_sha256']].append(entry['id'])
        if any(isinstance(scenario.get(side, {}).get('team'), str) for side in ('p1', 'p2')):
            raise ValueError(f'Team input is not inlined: {entry["id"]}')
        first = None
        position_before = set()
        for report in entry['reports']:
            data = record(report['file'], 'oracle_report', report['bytes'], report['gz_bytes'])
            if data['mode'] != report['mode'] or data.get('roll') != report.get('roll'):
                raise ValueError(f'Report mode/roll differs from index: {report["file"]}')
            # The oracle marks only Full as exact=true. Extremes/fixed are exact
            # comparisons of their reduced mode, not the full damage distribution.
            if (data.get('showdownCommit') != report['showdown']
                    or type(data.get('exact')) is not bool
                    or (data['mode'] == 'full' and data['exact'] is not True)):
                raise ValueError(f'Report provenance/exactness differs: {report["file"]}')
            if not isinstance(data['before'], dict) or not isinstance(data['outcomes'], list):
                raise ValueError('Missing report state/outcomes')
            if len(data['outcomes']) != report['outcomes']:
                raise ValueError(f'Report outcome count differs: {report["file"]}')
            for outcome in data['outcomes']:
                probability = outcome['p']
                if isinstance(probability, bool) or not isinstance(probability, (int, float)) or not math.isfinite(probability) or probability < 0 or not isinstance(outcome['state'], dict):
                    raise ValueError(f'Invalid oracle outcome: {report["file"]}')
            before_hash = digest(canonical(data['before']))
            position_before.add(before_hash)
            all_before.add(before_hash)
            modes[report['label']] += 1
            versions[report['showdown']] += 1
            job = {'id': entry['id'] + '/' + report['label'], 'position_id': entry['id'],
                   'scenario': entry['scenario'], 'report': report['file'], 'label': report['label'],
                   'mode': report['mode'], 'roll': report.get('roll'),
                   'staged': bool(report.get('staged')), 'before_sha256': before_hash,
                   'oracle_full_exact': data['exact'],
                   'oracle_outcomes': len(data['outcomes'])}
            oracle_jobs.append(job)
            if first is None:
                first = (job, data['before'])
        if first is None:
            raise ValueError(f'Included position has no reports: {entry["id"]}')
        first_job, before = first
        sides = before.get('sides', [])
        boundary = (entry['kind'] == 'Turn' and len(sides) == 2
                    and all(s.get('request') == 'move' for s in sides)
                    and not before.get('ended') and len(position_before) == 1)
        job = dict(first_job, id=entry['id'], **{key: entry[key] for key in
                   ('set', 'game', 'matchup', 'policy', 'kind', 'turn')})
        job.update(stage=stage(entry['turn']), before_features=features(before),
                   search_boundary_eligible=boundary,
                   report_before_variants=len(position_before),
                   has_recorded_mid_turn=bool(scenario.get('midTurn')))
        turn_jobs.append(job)
        before_groups[first_job['before_sha256']].append(entry['id'])
    expected_gz = {name for name in members if name.endswith('.gz')}
    actual_gz = {p.relative_to(corpus).as_posix() for p in (corpus/'positions').rglob('*.gz')}
    if actual_gz != expected_gz:
        raise ValueError('Gzip file inventory differs from the original index')
    selection, selection_audit = select_search(turn_jobs, search_size)
    manifest = {'schema_version': SCHEMA, 'source_corpus': str(corpus),
                'source_corpus_json_sha256': digest(original_index),
                'archive': ARCHIVE, 'source_files_unchanged': True,
                'members': dict(sorted(members.items())),
                'counts': {'entries': len(entries), 'positions': len(included),
                           'reports': len(oracle_jobs), 'excluded': len(entries)-len(included),
                           'gzip_files': len(expected_gz),
                           'gzip_bytes': sum(members[n]['bytes'] for n in expected_gz),
                           'uncompressed_gzip_bytes': sum(members[n]['uncompressed_bytes'] for n in expected_gz),
                           'unique_raw_scenarios': len(raw_scenarios),
                           'unique_first_report_before_states': len(before_groups),
                           'unique_all_report_before_states': len(all_before)},
                'report_modes': dict(sorted(modes.items())), 'showdown_commits': dict(versions),
                'excluded': [{'id': e['id'], 'reason': e.get('excluded')} for e in entries if e['build']=='excluded'],
                'duplicate_scenario_hashes': {k:v for k,v in sorted(raw_scenarios.items()) if len(v)>1},
                'duplicate_before_hashes': {k:v for k,v in sorted(before_groups.items()) if len(v)>1},
                'turn_jobs': turn_jobs, 'oracle_jobs': oracle_jobs, 'search_jobs': selection,
                'coverage': coverage(turn_jobs), 'search_selection': selection_audit,
                'selection_algorithm': 'v1: first report before, exact sorted-JSON SHA256 dedup; Turn + two move requests + not ended; matchup round-robin covering games first, then least-covered early/middle/late stage; prefer underrepresented turn numbers and new observed before features; stable ID SHA256 tie break',
                'stage_boundaries': {'early': 'turn <= 5', 'middle': '6 <= turn <= 15', 'late': 'turn >= 16'},
                'limitations': [
                    'Packaging only: no new engine or oracle execution; historical matches are not fresh variant validation.',
                    'Original gzip bytes and full corpus.json provenance are preserved; duplicate IDs are not deduplicated in the full check.',
                    'Search samples 256 unique canonical before states, not all hidden states or independent games.',
                    'Search boundary filter does not replace the Rust replay assertion that the selected position has no suspension.',
                    'Before-feature counts describe observed state fields, not complete engine coverage; old features.md is historical.',
                    'Canonical oracle parity uses lab-check probability tolerance 1e-9; internal state/Hash/instruction equivalence needs a separate differential probe.',
                    'Reduced-roll/fixed reports do not establish the corresponding Full distribution; six stale-pin positions remain excluded.',
                ]}
    out.mkdir(parents=True, exist_ok=False)
    archive_path = out/ARCHIVE
    with archive_path.open('xb') as destination:
        with gzip.GzipFile(filename='', mode='wb', fileobj=destination, mtime=0, compresslevel=9) as compressed:
            with tarfile.open(fileobj=compressed, mode='w', format=tarfile.PAX_FORMAT) as archive:
                for name, member in manifest['members'].items():
                    data = paths[name].read_bytes()
                    if digest(data) != member['sha256']:
                        raise ValueError(f'Original input changed while packaging: {name}')
                    info = tarfile.TarInfo(name)
                    info.size, info.mode, info.mtime = len(data), 0o644, 0
                    info.uid = info.gid = 0
                    info.uname = info.gname = ''
                    archive.addfile(info, io.BytesIO(data))
    # Independently read back every tar member; no extraction/path handling is needed here.
    with tarfile.open(archive_path, 'r:gz') as archive:
        items = archive.getmembers()
        if len(items) != len(members) or {item.name for item in items} != set(members):
            raise ValueError('Archive member inventory failed readback')
        for item in items:
            if not item.isfile() or digest(archive.extractfile(item).read()) != members[item.name]['sha256']:
                raise ValueError(f'Archive bytes failed readback: {item.name}')
    for name, path in paths.items():
        if digest(path.read_bytes()) != members[name]['sha256']:
            raise ValueError(f'Original input changed after packaging: {name}')
    manifest['archive_sha256'] = digest(archive_path.read_bytes())
    manifest['archive_bytes'] = archive_path.stat().st_size
    manifest['archive_readback_verified'] = True
    (out/'corpus-manifest.json').write_bytes(canonical(manifest)+b'\n')
    (out/'search-selection.json').write_bytes(canonical({'schema_version': SCHEMA,
        'source_corpus_json_sha256': manifest['source_corpus_json_sha256'],
        'selection_algorithm': manifest['selection_algorithm'], 'audit': selection_audit,
        'jobs': selection})+b'\n')
    return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--corpus', type=Path, required=True)
    parser.add_argument('--out-dir', type=Path, required=True)
    parser.add_argument('--search-size', type=int, default=256)
    args = parser.parse_args()
    manifest = make_pack(args.corpus, args.out_dir, args.search_size)
    print(json.dumps({key: manifest[key] for key in ('counts', 'archive_bytes', 'archive_sha256')}))


if __name__ == '__main__':
    main()
