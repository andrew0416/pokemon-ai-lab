"""Compare the seed's M-C mechanics assertions (fn15 delta, Showdown aa17ca0) with
lab-engine's champions.json + learnsets (Showdown 9e317a6, mod champions)."""
import json, sys, collections
sys.stdout.reconfigure(encoding='utf-8')

DELTA = r'D:\poke-teambuilder-seed\tools\seed-pipeline\out\historical-seed\2026-09-20-fn15-mc-engine\prepared1\delta.json'
REF = r'D:\pokemon-ai-lab\engine\data\champions.json'
LEARN_PATH = r'C:\Users\admin\AppData\Local\Temp\lab\champions-learnsets.json'
BINDING = 'binding:champions:ps-git-aa17ca0fac8bc5605df673bd8774c2d0e91efa43:mechanics:observed'

d = json.load(open(DELTA, encoding='utf-8'))
ref = json.load(open(REF, encoding='utf-8'))
LEARN = json.load(open(LEARN_PATH, encoding='utf-8'))
asserts = [a for a in d['rows']['assertions'] if a['bindingId'] == BINDING]
print('M-C assertions:', len(asserts))

species = ref['species']; moves = ref['moves']; items = ref['items']; abilities = ref['abilities']; types = ref['types']
TYPE_NAME = {k: v['name'] for k, v in types.items()}

def sid(subject): return subject.split(':', 2)[2]
def to_id(s): return ''.join(c for c in s.lower() if c.isalnum())
def champions_pp(m): return m['pp'] if m.get('noPPBoosts') else (m['pp'] // 5 + 1) * 4

mismatch = collections.Counter(); checked = collections.Counter(); examples = collections.defaultdict(list)
def cmp(dim, field, subj, seed_value, ref_value):
    checked[(dim, field)] += 1
    if seed_value != ref_value:
        mismatch[(dim, field)] += 1
        if len(examples[(dim, field)]) < 6:
            examples[(dim, field)].append((subj, seed_value, ref_value))

missing = collections.Counter(); seen_pairs = set(); unchecked = collections.Counter()
for a in asserts:
    dim, field, subj, st, val = a['dimension'], a['field'], a['subjectId'], a['state'], a['value']
    key = sid(subj)
    if dim == 'move_core':
        m = moves.get(key)
        if not m: missing[('move', key)] += 1; continue
        if field == 'type': cmp(dim, field, key, val, m['type'])
        elif field == 'category': cmp(dim, field, key, val, m['category'])
        elif field == 'basePower': cmp(dim, field, key, val, m['basePower'])
        elif field == 'accuracy':
            sv = 'always' if val.get('kind') == 'always' else val.get('value')
            rv = 'always' if m['accuracy'] is True else m['accuracy']
            cmp(dim, field, key, sv, rv)
        elif field == 'pp':
            cmp(dim, 'pp.initial(champions max)', key, val.get('initial'), champions_pp(m))
            cmp(dim, 'pp.resolved', key, val.get('resolved'), champions_pp(m))
        elif field == 'priority': cmp(dim, field, key, val, m['priority'])
        elif field == 'target': cmp(dim, field, key, val, m['target'])
        elif field == 'effect': unchecked[(dim, field)] += 1
        else: unchecked[(dim, field)] += 1
    elif dim == 'species_stats':
        s = species.get(key)
        if not s: missing[('species', key)] += 1; continue
        if field == 'baseStats': cmp(dim, field, key, val, s['baseStats'])
        elif field == 'types': cmp(dim, field, key, val, s['types'])
        elif field == 'formRelation':
            keys = ['num', 'name', 'baseSpecies', 'battleOnly', 'changesFrom', 'requiredAbility', 'requiredMove']
            rv = {k: s.get(k) for k in keys}
            rv['forme'] = s.get('forme') or None
            rv['requiredItems'] = s.get('requiredItems') or []
            rv['otherFormes'] = s.get('otherFormes') or []
            rv['cosmeticFormes'] = s.get('cosmeticFormes') or []
            sv = {k: val.get(k) for k in rv}
            if sv['requiredItems'] is None: sv['requiredItems'] = []
            cmp(dim, field, key, sv, rv)
            cmp(dim, 'formRelation.isBaseForm', key, val.get('isBaseForm'), s['baseSpecies'] == s['name'])
        else: unchecked[(dim, field)] += 1
    elif dim == 'ability_slot':
        s = species.get(key)
        if not s: missing[('species', key)] += 1; continue
        slot = field.split(':')[1]
        ra = s['abilities'].get(slot)
        rv = ('subject:ability:' + to_id(ra)) if ra else None
        cmp(dim, 'slot', key + '/' + slot, val if st == 'known_value' else None, rv)
    elif dim == 'type_chart':
        t = types.get(key)
        if not t: missing[('type', key)] += 1; continue
        # seed: subject = defending type? field = other type. Try both orientations.
        code_def = t['damageTaken'].get(TYPE_NAME.get(field, field.capitalize()))
        att = types.get(field)
        code_att = att['damageTaken'].get(t['name']) if att else None
        def conv(code):
            return {0: {'effectiveness': 0, 'immune': False}, 1: {'effectiveness': 1, 'immune': False},
                    2: {'effectiveness': -1, 'immune': False}, 3: {'effectiveness': 0, 'immune': True}}.get(code)
        cmp(dim, 'subject=defender,field=attacker', key + '<-' + field, val, conv(code_def))
        cmp(dim, 'subject=attacker,field=defender', key + '->' + field, val, conv(code_att))
    elif dim == 'item_effect':
        it = items.get(key)
        if not it: missing[('item', key)] += 1; continue
        if field == 'availability': cmp(dim, field, key, val, 'standard' if it.get('isNonstandard') is None else it['isNonstandard'])
        elif field == 'formRequirement': cmp(dim, field, key, val if st == 'known_value' else None, {'itemUser': it['itemUser']} if it.get('itemUser') else None)
        elif field == 'transformationTarget':
            rv = None
            if it.get('megaStone'): rv = {'megaStone': it['megaStone']}
            cmp(dim, field, key, val if st == 'known_value' else None, rv)
        else: unchecked[(dim, field)] += 1
    elif dim == 'learn_route':
        ls = LEARN.get(key)
        if ls is None: missing[('learnset', key)] += 1; continue
        cmp(dim, 'codes', key + '/' + field, sorted(val.get('codes', [])), sorted(ls.get(field, [])))
        seen_pairs.add((key, field))
    elif dim == 'asset_ref':
        unchecked[(dim, field)] += 1
    else:
        unchecked[(dim, field)] += 1

seed_species = {sid(a['subjectId']) for a in asserts if a['dimension'] == 'species_stats' and a['field'] == 'types'}
ref_roster = {k for k, v in species.items() if v.get('isNonstandard') is None}
print('\nspecies: seed', len(seed_species), 'ref roster (isNonstandard null)', len(ref_roster),
      'only-seed', sorted(seed_species - ref_roster)[:20], 'only-ref', sorted(ref_roster - seed_species)[:20])
seed_moves = {sid(a['subjectId']) for a in asserts if a['dimension'] == 'move_core' and a['field'] == 'type'}
ref_moves = {k for k, v in moves.items() if v.get('isNonstandard') is None}
print('moves: seed', len(seed_moves), 'ref (isNonstandard null)', len(ref_moves), 'only-seed', sorted(seed_moves - ref_moves)[:20], 'only-ref', sorted(ref_moves - seed_moves)[:20])
seed_items = {sid(a['subjectId']) for a in asserts if a['dimension'] == 'item_effect' and a['field'] == 'availability'}
ref_items = {k for k, v in items.items() if v.get('isNonstandard') is None}
print('items: seed', len(seed_items), 'ref (isNonstandard null)', len(ref_items), 'only-seed', sorted(seed_items - ref_items)[:20], 'only-ref', sorted(ref_items - seed_items)[:20])
ref_pairs = {(sp, mv) for sp in seed_species for mv in LEARN.get(sp, {})}
print('learn_route: seed pairs', len(seen_pairs), 'ref pairs for same species', len(ref_pairs),
      'only-in-ref', len(ref_pairs - seen_pairs), 'only-in-seed', len(seen_pairs - ref_pairs))
print('  only-in-ref sample', sorted(ref_pairs - seen_pairs)[:10]); print('  only-in-seed sample', sorted(seen_pairs - ref_pairs)[:10])
print('\nchecked:'); [print('  ', k, v, 'mismatch', mismatch.get(k, 0)) for k, v in checked.items()]
print('unchecked (representation only):', dict(unchecked))
print('missing subjects:', dict(missing))
for k, ex in examples.items():
    print('\nMISMATCH', k)
    for e in ex: print('   ', e[0], '| seed', json.dumps(e[1], ensure_ascii=False)[:220], '| ref', json.dumps(e[2], ensure_ascii=False)[:220])
