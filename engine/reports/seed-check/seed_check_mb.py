"""M-B / M-A checks: (1) seed aa17ca0 championsregmb binding vs vendor championsregmb mod;
(2) seed npm-0.11.11 `champions` binding (labeled M-B) vs vendor championsregmb and champions;
(3) official rosters (fn6) vs engine rosters."""
import json, sys, collections
sys.stdout.reconfigure(encoding='utf-8')
T = 'C:/Users/admin/AppData/Local/Temp/lab/'
DELTA = r'D:\poke-teambuilder-seed\tools\seed-pipeline\out\historical-seed\2026-09-20-fn15-mc-engine\prepared1\delta.json'
regmb = json.load(open(T + 'championsregmb-ref.json', encoding='utf-8'))
champ = json.load(open(T + 'champions-ref.json', encoding='utf-8'))
npm = json.load(open(T + 'seed-npm-champions.json', encoding='utf-8'))
rosters = json.load(open(T + 'seed-official-rosters.json', encoding='utf-8'))
delta = json.load(open(DELTA, encoding='utf-8'))

def sid(s): return s.split(':', 2)[2]
def to_id(s): return ''.join(c for c in s.lower() if c.isalnum())
def champions_pp(pp): return (pp // 5 + 1) * 4

def compare(label, asserts, ref):
    mism = collections.Counter(); chk = collections.Counter(); ex = collections.defaultdict(list); missing = collections.Counter()
    def cmp(k, subj, sv, rv):
        chk[k] += 1
        if sv != rv:
            mism[k] += 1
            if len(ex[k]) < 5: ex[k].append((subj, sv, rv))
    seen = set()
    for a in asserts:
        dim, field, st, val, key = a['dimension'], a['field'], a['state'], a['value'], sid(a['subjectId'])
        if dim == 'move_core':
            m = ref['moves'].get(key)
            if not m: missing['move:' + key] += 1; continue
            if field in ('type', 'category', 'basePower', 'priority', 'target'): cmp(('move', field), key, val, m[field])
            elif field == 'accuracy': cmp(('move', field), key, 'always' if val.get('kind') == 'always' else val.get('value'), 'always' if m['accuracy'] is True else m['accuracy'])
            elif field == 'pp':
                cmp(('move', 'pp.resolved=base'), key, val.get('resolved'), m['pp'])
                cmp(('move', 'pp.initial=champions max'), key, val.get('initial'), champions_pp(m['pp']))
        elif dim == 'species_stats':
            s = ref['species'].get(key)
            if not s: missing['species:' + key] += 1; continue
            if field in ('types', 'baseStats'): cmp(('species', field), key, val, s[field])
        elif dim == 'ability_slot':
            s = ref['species'].get(key)
            if not s: missing['species:' + key] += 1; continue
            slot = field.split(':')[1]; ra = s['abilities'].get(slot)
            cmp(('ability', 'slot'), key + '/' + slot, val if st == 'known_value' else None, ('subject:ability:' + to_id(ra)) if ra else None)
        elif dim == 'learn_route':
            ls = ref['learnsets'].get(key)
            if ls is None: missing['learnset:' + key] += 1; continue
            cmp(('learn', 'codes'), key + '/' + field, sorted(val.get('codes', [])), sorted(ls.get(field, [])))
            seen.add((key, field))
        elif dim == 'item_effect' and field == 'transformationTarget':
            it = ref['items'].get(key)
            if it is None: missing['item:' + key] += 1; continue
            cmp(('item', field), key, val if st == 'known_value' else None, {'megaStone': it['megaStone']} if it['megaStone'] else None)
    seed_species = {sid(a['subjectId']) for a in asserts if a['dimension'] == 'species_stats' and a['field'] == 'types'}
    seed_moves = {sid(a['subjectId']) for a in asserts if a['dimension'] == 'move_core' and a['field'] == 'type'}
    seed_items = {sid(a['subjectId']) for a in asserts if a['dimension'] == 'item_effect'}
    ref_pairs = {(sp, mv) for sp in seed_species for mv in ref['learnsets'].get(sp, {})}
    print(f'\n===== {label}')
    print('species seed', len(seed_species), 'ref', len(ref['species']), 'only-seed', sorted(seed_species - set(ref['species']))[:15], 'only-ref', sorted(set(ref['species']) - seed_species)[:15])
    print('moves seed', len(seed_moves), 'ref', len(ref['moves']), 'only-seed', sorted(seed_moves - set(ref['moves']))[:15], 'only-ref', sorted(set(ref['moves']) - seed_moves)[:15])
    print('items seed', len(seed_items), 'ref', len(ref['items']), 'only-seed', sorted(seed_items - set(ref['items']))[:15], 'only-ref', sorted(set(ref['items']) - seed_items)[:15])
    print('learn pairs seed', len(seen), 'ref(same species)', len(ref_pairs), 'only-ref', len(ref_pairs - seen), 'only-seed', len(seen - ref_pairs))
    if ref_pairs - seen: print('   only-ref sample', sorted(ref_pairs - seen)[:8])
    if seen - ref_pairs: print('   only-seed sample', sorted(seen - ref_pairs)[:8])
    for k, v in chk.items(): print('  ', k, v, 'mismatch', mism.get(k, 0))
    if missing: print('  missing subjects', dict(list(missing.items())[:15]), 'total', sum(missing.values()))
    for k, e in ex.items():
        print('  MISMATCH', k)
        for x in e: print('     ', x[0], '| seed', json.dumps(x[1], ensure_ascii=False)[:150], '| ref', json.dumps(x[2], ensure_ascii=False)[:150])

B_MB = 'binding:championsregmb:ps-git-aa17ca0fac8bc5605df673bd8774c2d0e91efa43:mechanics:observed'
compare('seed aa17ca0 championsregmb (M-B) vs vendor championsregmb', [a for a in delta['rows']['assertions'] if a['bindingId'] == B_MB], regmb)
compare('seed npm-0.11.11 champions (labeled M-B) vs vendor championsregmb', npm['binding:champions:mechanics:observed'], regmb)
compare('seed npm-0.11.11 champions (labeled M-B) vs vendor champions (M-C)', npm['binding:champions:mechanics:observed'], champ)

print('\n===== official rosters vs engine rosters (species number level)')
for key, ref, name in [('M-B', regmb, 'championsregmb'), ('M-C', champ, 'champions')]:
    v = rosters[f'assertion:official:champions:reg:{key}:eligibleSpecies:v1']
    off = {int(e['key'].split('-')[0]) for e in v['entries']}
    eng = {s['num'] for s in ref['species'].values()}
    eng_base = {s['num'] for s in ref['species'].values() if s['baseSpecies'] == s['name'] or not s['forme'].startswith('Mega')}
    print(key, 'official entries', v['count'], 'nums', len(off), '| engine', name, 'forms', len(ref['species']), 'nums', len(eng))
    print('   official-not-engine', sorted(off - eng), '| engine-not-official', [(s['num'], s['name']) for s in ref['species'].values() if s['num'] in eng - off])
ma = rosters['assertion:official:champions:reg:M-A:eligibleSpecies:v1']
npm_ma = {sid(a['subjectId']) for a in npm['binding:championsregma:mechanics:observed'] if a['dimension'] == 'species_stats' and a['field'] == 'types'}
print('M-A official entries', ma['count'], '| seed npm regma species/forms', len(npm_ma))
