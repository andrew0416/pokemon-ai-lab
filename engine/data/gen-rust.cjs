'use strict';
// Generates lab-engine's static dex tables (engine/core/src/dex/generated.rs) from
// engine/data/champions.json.
//
// The generator is strict: every field of the export is either converted or listed as
// deliberately dropped (legality, flavour, evolution data). An unknown field, value shape
// or name reference fails the run, so a newer Showdown export cannot silently lose data.
//
// Layout of the generated tables:
// - Index 0 of every table is the "none" entry (no item, empty move slot, ...), so
//   `Default` ids are valid and `data()` never needs an Option.
// - Entries are sorted by Showdown id, so `from_id` is a binary search.
// - Every entry also gets a constant named after its display name (`moves::HYPNOSIS`,
//   `species::GARDEVOIR_MEGA`). Hand-written rules refer to these constants; if a later
//   export drops an entry, the rules code stops compiling instead of misbehaving.
//
// Usage: node engine/data/gen-rust.cjs [--check]
//   --check  exit 1 if the generated file is out of date (CI).

const fs = require('node:fs');
const path = require('node:path');

const DATA = path.join(__dirname, 'champions.json');
const OUT = path.join(__dirname, '../core/src/dex/generated.rs');

const data = JSON.parse(fs.readFileSync(DATA, 'utf8'));

// ---------------------------------------------------------------------------------------
// Helpers

function fail(msg) {
	throw new Error(msg);
}

const toId = s => String(s).toLowerCase().replace(/[^a-z0-9]+/g, '');

// Display name -> SCREAMING_SNAKE constant name.
function constName(name) {
	const s = String(name)
		.replace(/♀/g, '-F').replace(/♂/g, '-M')
		.normalize('NFD').replace(/[̀-ͯ]/g, '')
		.replace(/[’'.,%:]/g, '')
		.replace(/[^A-Za-z0-9]+/g, '_')
		.replace(/^_+|_+$/g, '')
		.toUpperCase();
	if (!s) fail(`empty constant name for ${JSON.stringify(name)}`);
	return /^[0-9]/.test(s) ? `N${s}` : s;
}

const camel = s => s.replace(/(^|[^A-Za-z0-9])([a-z])/g, (_, __, c) => c.toUpperCase()).replace(/[^A-Za-z0-9]/g, '');
const str = s => JSON.stringify(String(s));
const bool = b => (b ? 'true' : 'false');

function int(v, min, max, what) {
	if (!Number.isInteger(v) || v < min || v > max) fail(`${what}: ${JSON.stringify(v)} not an integer in ${min}..=${max}`);
	return String(v);
}

// Checks that every key of `obj` is handled or dropped.
function checkKeys(obj, handled, dropped, what) {
	for (const k of Object.keys(obj)) {
		if (handled.has(k) || dropped.has(k) || isEventOrder(k)) continue;
		fail(`${what}: unknown field ${k} = ${JSON.stringify(obj[k]).slice(0, 120)}`);
	}
}

// Event ordering constants (onResidualOrder, onBasePowerPriority, ...). Values, not
// callbacks; kept so the turn engine can order hand-written handlers the way Showdown does.
// `onFractionalPriority` is a value (the -0.1 of Lagging Tail/Stall), not an order.
const isEventOrder = k => /^on[A-Z]\w*(Priority|Order|SubOrder)$/.test(k) && k !== 'onFractionalPriority';

function eventOrders(obj, prefix = '') {
	const out = [];
	for (const [k, v] of Object.entries(obj)) {
		if (isEventOrder(k)) out.push(`(${str(prefix + k)}, ${int(v, -32768, 32767, k)})`);
	}
	return out;
}

const list = items => (items.length ? `&[${items.join(', ')}]` : '&[]');

// Default field values of each table struct. The generated `NONE` constant (index 0 of
// every table) is built from these, and entries omit fields equal to the default
// (`..MoveData::NONE`), which keeps the generated file a fraction of the size.
const NO_BOOSTS = 'NO_BOOSTS';
const DEFAULTS = {
	SpeciesData: {
		id: '""', name: '""', num: '0', nonstandard: 'Nonstandard::None', base_species: 'SpeciesId::NONE', forme: '""',
		types: '[Type::None, Type::None]', base_stats: '[0, 0, 0, 0, 0, 0]',
		abilities: '[AbilityId::NONE, AbilityId::NONE, AbilityId::NONE, AbilityId::NONE]', weight_hg: '0',
		gender: 'Gender::Random', nfe: 'false', is_mega: 'false', is_primal: 'false', gigantamax_move: 'MoveId::NONE',
		cannot_dynamax: 'false', battle_only: '&[]', changes_from: 'SpeciesId::NONE', required_items: '&[]',
		required_ability: 'AbilityId::NONE', required_move: 'MoveId::NONE', required_tera_type: 'Type::None',
		fixed_max_hp: '0', event_orders: '&[]', handlers: '&[]',
	},
	MoveData: {
		id: '""', name: '""', num: '0', nonstandard: 'Nonstandard::None', move_type: 'Type::None',
		category: 'MoveCategory::Status', base_power: '0', accuracy: 'None', pp: '0', priority: '0',
		target: 'MoveTarget::Normal', non_ghost_target: 'None', flags: 'MoveFlags(0)', crit_ratio: '1', multihit: 'None',
		drain: 'None', recoil: 'None', heal: 'None', fixed_damage: 'None', ohko: 'Ohko::No',
		ignore_immunity: 'IgnoreImmunity::No', status: 'Status::None', volatile_status: 'ConditionId::NONE',
		side_condition: 'ConditionId::NONE', slot_condition: 'ConditionId::NONE', pseudo_weather: 'ConditionId::NONE',
		weather: 'ConditionId::NONE', terrain: 'ConditionId::NONE', boosts: NO_BOOSTS, self_effect: 'None',
		self_boost: NO_BOOSTS, secondaries: '&[]', condition_duration: '0', condition_counter_max: '0',
		condition_locks_move: 'false', condition_no_invulnerability: 'false', condition_blocks_crits: 'false',
		self_switch: 'SelfSwitch::No', selfdestruct: 'SelfDestruct::No', override_offensive_pokemon_target: 'false',
		override_offensive_stat: 'None', override_defensive_stat: 'None', z_move: 'ZMoveData::NONE', max_move_power: '0',
		will_crit: 'false', multiaccuracy: 'false', force_switch: 'false', breaks_protect: 'false', stalling_move: 'false',
		thaws_target: 'false', tracks_target: 'false', smart_target: 'false', sleep_usable: 'false', steals_boosts: 'false',
		calls_move: 'false', has_crash_damage: 'false', mind_blown_recoil: 'false', struggle_recoil: 'false',
		chloroblast_recoil: 'false', ignore_ability: 'false', ignore_evasion: 'false', ignore_defensive: 'false',
		ignore_offensive: 'false', ignore_negative_offensive: 'false', ignore_positive_defensive: 'false',
		has_sheer_force_boost: 'false', force_stab: 'false', no_pp_boosts: 'false', is_z: 'false', is_max: 'false',
		event_orders: '&[]', handlers: '&[]',
	},
	ItemData: {
		id: '""', name: '""', num: '0', nonstandard: 'Nonstandard::None', fling: 'None', is_berry: 'false',
		is_gem: 'false', is_choice: 'false', is_pokeball: 'false', is_primal_orb: 'false', ignore_klutz: 'false',
		cannot_be_taken: 'false', no_eat_effect: 'false', no_negate_immunity: 'false', fractional_priority_tenths: '0',
		mega_stone: '&[]', item_users: '&[]', forced_forme: 'SpeciesId::NONE', plate_type: 'Type::None',
		memory_type: 'Type::None', drive_type: 'Type::None', z_crystal: 'None', boosts: NO_BOOSTS, natural_gift: 'None',
		condition_duration: '0', event_orders: '&[]', handlers: '&[]',
	},
	AbilityData: {
		id: '""', name: '""', num: '0', nonstandard: 'Nonstandard::None', flags: 'AbilityFlags(0)',
		suppress_weather: 'false', cannot_be_crit: 'false', fractional_priority_tenths: '0', event_orders: '&[]',
		handlers: '&[]',
	},
	ConditionData: {id: '""', exported: 'false', duration: '0', counter_max: '0', event_orders: '&[]', handlers: '&[]'},
};

// One table entry from `name: expr` strings, omitting default-valued fields.
function record(typeName, fields) {
	const defaults = DEFAULTS[typeName];
	const seen = new Set();
	const kept = [];
	for (const f of fields) {
		const i = f.indexOf(': ');
		const name = f.slice(0, i);
		if (!(name in defaults)) fail(`${typeName}: field ${name} has no default`);
		seen.add(name);
		if (f.slice(i + 2) !== defaults[name]) kept.push(f);
	}
	for (const name of Object.keys(defaults)) if (!seen.has(name)) fail(`${typeName}: field ${name} not emitted`);
	const rest = kept.length === Object.keys(defaults).length ? [] : [`..${typeName}::NONE`];
	return `    ${typeName} { ${[...kept, ...rest].join(', ')} },`;
}

function genNoneConsts() {
	const out = [`pub const ${NO_BOOSTS}: Boosts = [0; BOOST_COUNT];`, ''];
	for (const [typeName, defaults] of Object.entries(DEFAULTS)) {
		out.push(`impl ${typeName} {`);
		out.push(`    pub const NONE: ${typeName} = ${typeName} {`);
		for (const [k, v] of Object.entries(defaults)) out.push(`        ${k}: ${v},`);
		out.push('    };');
		out.push('}');
		out.push('');
	}
	return out;
}

// ---------------------------------------------------------------------------------------
// Tables with ids

function makeTable(kind, entries) {
	const sorted = Object.keys(entries).sort();
	const index = new Map(sorted.map((id, i) => [id, i + 1]));
	const names = new Map();
	for (const id of sorted) {
		const c = constName(entries[id].name);
		if (names.has(c)) fail(`${kind}: constant ${c} for both ${names.get(c)} and ${id}`);
		names.set(c, id);
	}
	const constOf = new Map([...names].map(([c, id]) => [id, c]));
	return {kind, sorted, index, names, constOf, entries};
}

// Entries that cannot run under Champions mechanics at all. Anything else outside the
// regulation stays in (scope rule: the rules limit the engine, not the roster).
const EXCLUDED_SPECIES = {
	missingno: 'type "Bird" has no entry in the Champions type chart',
};
for (const id of Object.keys(EXCLUDED_SPECIES)) {
	if (!data.species[id]) fail(`excluded species ${id} is no longer exported; remove it from EXCLUDED_SPECIES`);
}
const includedSpecies = Object.fromEntries(Object.entries(data.species).filter(([id]) => !(id in EXCLUDED_SPECIES)));

const tables = {
	species: makeTable('species', includedSpecies),
	moves: makeTable('moves', data.moves),
	items: makeTable('items', data.items),
	abilities: makeTable('abilities', data.abilities),
};

function ref(table, name, what) {
	if (name === undefined || name === null || name === '') return `${idType[table.kind]}::NONE`;
	const i = table.index.get(toId(name));
	if (!i) fail(`${what}: ${table.kind} ${JSON.stringify(name)} not in the export`);
	return `${table.kind}::${table.constOf.get(toId(name))}`;
}

const idType = {species: 'SpeciesId', moves: 'MoveId', items: 'ItemId', abilities: 'AbilityId', conditions: 'ConditionId'};

// ---------------------------------------------------------------------------------------
// Types, stats, natures

const TYPES = Object.keys(data.types).sort();
const typeVariant = new Map(TYPES.map(id => [data.types[id].name, data.types[id].name]));

function type(name, what) {
	if (name === undefined || name === null || name === '') return 'Type::None';
	if (!typeVariant.has(name)) fail(`${what}: unknown type ${JSON.stringify(name)}`);
	return `Type::${name}`;
}

const STATS = {hp: 'Hp', atk: 'Atk', def: 'Def', spa: 'Spa', spd: 'Spd', spe: 'Spe'};
const stat = (s, what) => STATS[s] ? `Stat::${STATS[s]}` : fail(`${what}: unknown stat ${s}`);

// Order matches state::BOOST_COUNT: atk, def, spa, spd, spe, accuracy, evasion.
const BOOSTS = ['atk', 'def', 'spa', 'spd', 'spe', 'accuracy', 'evasion'];
function boosts(obj, what) {
	if (!obj) return 'NO_BOOSTS';
	for (const k of Object.keys(obj)) if (!BOOSTS.includes(k)) fail(`${what}: unknown boost ${k}`);
	return `[${BOOSTS.map(b => int(obj[b] ?? 0, -12, 12, `${what}.${b}`)).join(', ')}]`;
}

// Type-level immunities that are not attacking types (damageTaken keys in lower case).
const TYPE_IMMUNITY_KEYS = ['brn', 'frz', 'par', 'psn', 'tox', 'powder', 'prankster', 'sandstorm', 'hail', 'trapped'];

function genTypes() {
	const out = [];
	out.push('/// Pokémon types. `None` fills the second slot of a single-typed Pokémon.');
	out.push('#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]');
	out.push('#[repr(u8)]');
	out.push('pub enum Type {');
	out.push('    #[default]');
	out.push('    None = 0,');
	for (const id of TYPES) out.push(`    ${data.types[id].name},`);
	out.push('}');
	out.push('');
	out.push(`pub const TYPE_COUNT: usize = ${TYPES.length + 1};`);
	out.push('');
	out.push('impl Type {');
	out.push(`    pub const ALL: [Type; ${TYPES.length}] = [${TYPES.map(id => `Type::${data.types[id].name}`).join(', ')}];`);
	out.push('');
	out.push('    pub const fn name(self) -> &\'static str {');
	out.push('        match self {');
	out.push('            Type::None => "",');
	for (const id of TYPES) out.push(`            Type::${data.types[id].name} => ${str(data.types[id].name)},`);
	out.push('        }');
	out.push('    }');
	out.push('');
	out.push('    /// Parses a Showdown type name or id (case-insensitive).');
	out.push('    pub fn from_name(name: &str) -> Option<Type> {');
	out.push('        let id = super::to_id(name);');
	out.push('        Type::ALL.into_iter().find(|t| super::to_id(t.name()) == id)');
	out.push('    }');
	out.push('}');
	out.push('');

	// TYPE_CHART[defending][attacking], from Showdown's damageTaken codes.
	const code = {0: 'Neutral', 1: 'Super', 2: 'Resist', 3: 'Immune'};
	const row = (defId) => {
		const taken = defId ? data.types[defId].damageTaken : {};
		const cells = ['TypeRelation::Neutral'];
		for (const atkId of TYPES) {
			const v = defId ? taken[data.types[atkId].name] : 0;
			if (!(v in code)) fail(`type chart ${defId} <- ${atkId}: ${v}`);
			cells.push(`TypeRelation::${code[v]}`);
		}
		return `    [${cells.join(', ')}],`;
	};
	out.push('/// `TYPE_CHART[defending][attacking]`. The `None` row and column are neutral.');
	out.push('pub static TYPE_CHART: [[TypeRelation; TYPE_COUNT]; TYPE_COUNT] = [');
	out.push(row(null));
	for (const id of TYPES) out.push(row(id));
	out.push('];');
	out.push('');

	out.push('/// Non-type immunities a type grants (Electric: paralysis, Grass: powder, ...).');
	out.push('pub static TYPE_IMMUNITIES: [TypeImmunities; TYPE_COUNT] = [');
	out.push('    TypeImmunities::EMPTY,');
	for (const id of TYPES) {
		const bits = [];
		for (const [k, v] of Object.entries(data.types[id].damageTaken)) {
			if (/^[A-Z]/.test(k)) {
				if (!typeVariant.has(k)) fail(`type ${id}: unknown attacking type ${k}`);
				continue;
			}
			if (!TYPE_IMMUNITY_KEYS.includes(k)) fail(`type ${id}: unknown damageTaken key ${k}`);
			if (v === 3) bits.push(`TypeImmunities::${k.toUpperCase()}.bits()`);
			else if (v !== 0) fail(`type ${id}: ${k} = ${v}`);
		}
		out.push(`    TypeImmunities(${bits.length ? bits.join(' | ') : '0'}),`);
	}
	out.push('];');
	out.push('');
	out.push('impl TypeImmunities {');
	TYPE_IMMUNITY_KEYS.forEach((k, i) => out.push(`    pub const ${k.toUpperCase()}: TypeImmunities = TypeImmunities(1 << ${i});`));
	out.push('}');
	out.push('');
	return out;
}

function genNatures() {
	const ids = Object.keys(data.natures).sort();
	const out = [];
	out.push('#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]');
	out.push('#[repr(u8)]');
	out.push('pub enum Nature {');
	for (const id of ids) out.push(`    ${data.natures[id].name},`);
	out.push('}');
	out.push('');
	out.push('impl Nature {');
	out.push(`    pub const ALL: [Nature; ${ids.length}] = [${ids.map(id => `Nature::${data.natures[id].name}`).join(', ')}];`);
	out.push('');
	out.push('    pub const fn name(self) -> &\'static str {');
	out.push('        match self {');
	for (const id of ids) out.push(`            Nature::${data.natures[id].name} => ${str(data.natures[id].name)},`);
	out.push('        }');
	out.push('    }');
	out.push('');
	out.push('    /// (raised stat, lowered stat); both `None` for neutral natures.');
	out.push('    pub const fn modifiers(self) -> (Option<Stat>, Option<Stat>) {');
	out.push('        match self {');
	for (const id of ids) {
		const n = data.natures[id];
		if (!n.plus !== !n.minus) fail(`nature ${id}: plus/minus mismatch`);
		const opt = s => (s ? `Some(${stat(s, `nature ${id}`)})` : 'None');
		out.push(`            Nature::${n.name} => (${opt(n.plus)}, ${opt(n.minus)}),`);
	}
	out.push('        }');
	out.push('    }');
	out.push('}');
	out.push('');
	return out;
}

// ---------------------------------------------------------------------------------------
// Conditions: every status/volatile/side/field condition that data refers to by name.

const conditionIds = new Set(Object.keys(data.conditions));
function noteCondition(name) {
	if (name) conditionIds.add(toId(name));
}
for (const m of Object.values(data.moves)) {
	for (const k of ['volatileStatus', 'sideCondition', 'slotCondition', 'pseudoWeather', 'weather', 'terrain']) noteCondition(m[k]);
	for (const k of ['volatileStatus', 'sideCondition', 'pseudoWeather']) noteCondition(m.self?.[k]);
	for (const s of m.secondaries ?? []) noteCondition(s.volatileStatus);
}
for (const i of Object.values(data.items)) noteCondition(i.fling?.volatileStatus);
const CONDITIONS = [...conditionIds].sort();
if (CONDITIONS.length > 255) fail('more than 255 conditions');
const conditionIndex = new Map(CONDITIONS.map((id, i) => [id, i + 1]));
const condition = name => (name ? `conditions::${toId(name).toUpperCase()}` : 'ConditionId::NONE');

function genConditions() {
	const out = [];
	out.push('pub mod conditions {');
	out.push('    use super::ConditionId;');
	CONDITIONS.forEach((id, i) => out.push(`    pub const ${id.toUpperCase()}: ConditionId = ConditionId(${i + 1});`));
	out.push('}');
	out.push('');
	const handled = new Set(['name', 'id', 'num', 'isNonstandard', 'affectsFainted', 'handlers', 'duration', 'counterMax']);
	out.push(`pub static CONDITIONS: [ConditionData; ${CONDITIONS.length + 1}] = [`);
	out.push('    ConditionData::NONE,');
	for (const id of CONDITIONS) {
		const c = data.conditions[id];
		if (c) checkKeys(c, handled, new Set(), `condition ${id}`);
		out.push(record('ConditionData', [`id: ${str(id)}`, `exported: ${bool(!!c)}`, `duration: ${int(c?.duration ?? 0, 0, 255, id)}`, `counter_max: ${int(c?.counterMax ?? 0, 0, 65535, id)}`,
			`event_orders: ${list(c ? eventOrders(c) : [])}`, `handlers: ${list((c?.handlers ?? []).map(str))}`]));
	}
	out.push('];');
	out.push('');
	return out;
}

// ---------------------------------------------------------------------------------------
// Species

const NONSTANDARD = {Past: 'Past', Future: 'Future', LGPE: 'Lgpe', Gmax: 'Gmax', CAP: 'Cap', Custom: 'Custom', Unobtainable: 'Unobtainable'};
const nonstandard = (v, what) => (v === null || v === undefined ? 'Nonstandard::None'
	: NONSTANDARD[v] ? `Nonstandard::${NONSTANDARD[v]}` : fail(`${what}: isNonstandard ${v}`));

const SPECIES_DROPPED = new Set([
	// Bookkeeping / legality / flavour / breeding / evolution: the validator's business.
	'affectsFainted', 'baseForme', 'prevo', 'evos', 'evoLevel', 'evoCondition', 'evoType', 'evoItem', 'evoRegion',
	'evoMove', 'eggGroups', 'canHatch', 'genderRatio', 'bst', 'weightkg', 'heightm', 'color', 'tags',
	'unreleasedHidden', 'maleOnlyHidden', 'gmaxUnreleased', 'otherFormes', 'formeOrder', 'cosmeticFormes',
	'isCosmeticForme', 'placeholderFor', 'mother',
	// Superseded by requiredItems (always a superset).
	'requiredItem',
]);
const SPECIES_HANDLED = new Set(['name', 'id', 'num', 'isNonstandard', 'baseSpecies', 'forme', 'abilities', 'types',
	'nfe', 'gender', 'baseStats', 'weighthg', 'canGigantamax', 'cannotDynamax', 'requiredItems', 'isMega', 'isPrimal',
	'battleOnly', 'changesFrom', 'requiredAbility', 'requiredMove', 'requiredTeraType', 'maxHP', 'handlers']);
const GENDER = {'': 'Gender::Random', M: 'Gender::Male', F: 'Gender::Female', N: 'Gender::Genderless'};

function genSpecies() {
	const t = tables.species;
	const out = [];
	out.push(`pub static SPECIES: [SpeciesData; ${t.sorted.length + 1}] = [`);
	out.push('    SpeciesData::NONE,');
	for (const id of t.sorted) {
		const s = t.entries[id];
		const w = `species ${id}`;
		checkKeys(s, SPECIES_HANDLED, SPECIES_DROPPED, w);
		if (s.requiredItem && !(s.requiredItems ?? []).includes(s.requiredItem)) fail(`${w}: requiredItem not in requiredItems`);
		if (s.types.length < 1 || s.types.length > 2) fail(`${w}: types`);
		for (const k of Object.keys(s.abilities)) if (!['0', '1', 'H', 'S'].includes(k)) fail(`${w}: ability slot ${k}`);
		if (!(s.gender in GENDER)) fail(`${w}: gender ${s.gender}`);
		const b = s.baseStats;
		const battleOnly = s.battleOnly === undefined ? [] : [].concat(s.battleOnly);
		out.push(record('SpeciesData', [
			`id: ${str(id)}`, `name: ${str(s.name)}`, `num: ${int(s.num, -32768, 32767, w)}`,
			`nonstandard: ${nonstandard(s.isNonstandard, w)}`,
			`base_species: ${ref(t, s.baseSpecies, w)}`, `forme: ${str(s.forme)}`,
			`types: [${type(s.types[0], w)}, ${type(s.types[1], w)}]`,
			`base_stats: [${['hp', 'atk', 'def', 'spa', 'spd', 'spe'].map(k => int(b[k], 1, 255, `${w}.${k}`)).join(', ')}]`,
			`abilities: [${['0', '1', 'H', 'S'].map(k => ref(tables.abilities, s.abilities[k], w)).join(', ')}]`,
			`weight_hg: ${int(s.weighthg, 0, 65535, w)}`, `gender: ${GENDER[s.gender]}`,
			`nfe: ${bool(s.nfe)}`, `is_mega: ${bool(s.isMega)}`, `is_primal: ${bool(s.isPrimal)}`,
			`gigantamax_move: ${ref(tables.moves, s.canGigantamax, w)}`, `cannot_dynamax: ${bool(s.cannotDynamax)}`,
			`battle_only: ${list(battleOnly.map(n => ref(t, n, w)))}`, `changes_from: ${ref(t, s.changesFrom, w)}`,
			`required_items: ${list((s.requiredItems ?? []).map(n => ref(tables.items, n, w)))}`,
			`required_ability: ${ref(tables.abilities, s.requiredAbility, w)}`,
			`required_move: ${ref(tables.moves, s.requiredMove, w)}`,
			`required_tera_type: ${type(s.requiredTeraType, w)}`,
			`fixed_max_hp: ${int(s.maxHP ?? 0, 0, 65535, w)}`,
			`event_orders: ${list(eventOrders(s))}`, `handlers: ${list((s.handlers ?? []).map(str))}`,
		]));
	}
	out.push('];');
	out.push('');
	return out;
}

// ---------------------------------------------------------------------------------------
// Moves

const TARGETS = ['normal', 'any', 'adjacentAlly', 'adjacentAllyOrSelf', 'adjacentFoe', 'allAdjacent',
	'allAdjacentFoes', 'allies', 'allySide', 'allyTeam', 'all', 'foeSide', 'randomNormal', 'scripted', 'self'];
// `self` is a Rust keyword, so that target is `MoveTarget::User`.
const target = (v, what) => (!TARGETS.includes(v) ? fail(`${what}: target ${v}`)
	: v === 'self' ? 'MoveTarget::User' : `MoveTarget::${camel(v)}`);

const MOVE_DROPPED = new Set(['affectsFainted', 'spreadHit', 'contestType', 'tags', 'placeholderFor',
	// `secondary` is always secondaries[0] (or absent), except when Showdown lists several.
	'secondary']);
const MOVE_BOOL = {
	willCrit: 'will_crit', multiaccuracy: 'multiaccuracy', forceSwitch: 'force_switch', breaksProtect: 'breaks_protect',
	stallingMove: 'stalling_move', thawsTarget: 'thaws_target', tracksTarget: 'tracks_target', smartTarget: 'smart_target',
	sleepUsable: 'sleep_usable', stealsBoosts: 'steals_boosts', callsMove: 'calls_move', hasCrashDamage: 'has_crash_damage',
	mindBlownRecoil: 'mind_blown_recoil', struggleRecoil: 'struggle_recoil', chloroblastRecoil: 'chloroblast_recoil',
	ignoreAbility: 'ignore_ability', ignoreEvasion: 'ignore_evasion', ignoreDefensive: 'ignore_defensive',
	ignoreOffensive: 'ignore_offensive', ignoreNegativeOffensive: 'ignore_negative_offensive',
	ignorePositiveDefensive: 'ignore_positive_defensive', hasSheerForceBoost: 'has_sheer_force_boost',
	forceSTAB: 'force_stab', noPPBoosts: 'no_pp_boosts', isZ: 'is_z', isMax: 'is_max',
};
const MOVE_HANDLED = new Set(['name', 'id', 'num', 'isNonstandard', 'type', 'category', 'basePower', 'accuracy', 'pp',
	'priority', 'target', 'nonGhostTarget', 'flags', 'critRatio', 'multihit', 'drain', 'recoil', 'heal', 'damage', 'ohko',
	'status', 'volatileStatus', 'sideCondition', 'slotCondition', 'pseudoWeather', 'weather', 'terrain', 'boosts', 'self',
	'selfBoost', 'secondaries', 'condition', 'selfSwitch', 'selfdestruct', 'ignoreImmunity', 'overrideOffensivePokemon',
	'overrideOffensiveStat', 'overrideDefensiveStat', 'zMove', 'maxMove', 'handlers', ...Object.keys(MOVE_BOOL)]);

const MOVE_FLAGS = [...new Set(Object.values(data.moves).flatMap(m => Object.keys(m.flags)))].sort();
if (MOVE_FLAGS.length > 64) fail('more than 64 move flags');

const STATUS = {brn: 'Status::Burn', frz: 'Status::Freeze', par: 'Status::Paralyze', psn: 'Status::Poison',
	tox: 'Status::Toxic', slp: 'Status::Sleep'};
const status = (v, what) => (!v ? 'Status::None' : STATUS[v] ?? fail(`${what}: status ${v}`));

function fraction(v, what) {
	if (!v) return 'None';
	if (!Array.isArray(v) || v.length !== 2) fail(`${what}: fraction ${JSON.stringify(v)}`);
	return `Some(Fraction(${int(v[0], 1, 255, what)}, ${int(v[1], 1, 255, what)}))`;
}

function flagsExpr(flags, all, typeName, what) {
	const bits = Object.keys(flags ?? {}).map(f => {
		const i = all.indexOf(f);
		if (i < 0) fail(`${what}: unknown flag ${f}`);
		return `${typeName}::${f.toUpperCase()}.bits()`;
	});
	return `${typeName}(${bits.length ? bits.join(' | ') : '0'})`;
}

function secondaryExpr(s, what) {
	checkKeys(s, new Set(['chance', 'status', 'volatileStatus', 'boosts', 'self']), new Set(), what);
	if (s.self) checkKeys(s.self, new Set(['boosts']), new Set(), `${what}.self`);
	return `Secondary { chance: ${int(s.chance ?? 100, 1, 100, what)}, status: ${status(s.status, what)}, ` +
		`volatile_status: ${condition(s.volatileStatus)}, boosts: ${boosts(s.boosts, what)}, ` +
		`self_boosts: ${boosts(s.self?.boosts, what)} }`;
}

function genMoves() {
	const t = tables.moves;
	const out = [];
	out.push(`pub static MOVES: [MoveData; ${t.sorted.length + 1}] = [`);
	out.push('    MoveData::NONE,');
	for (const id of t.sorted) {
		const m = t.entries[id];
		const w = `move ${id}`;
		checkKeys(m, MOVE_HANDLED, MOVE_DROPPED, w);
		if (m.secondary && JSON.stringify(m.secondary) !== JSON.stringify(m.secondaries?.[0])) fail(`${w}: secondary is not secondaries[0]`);
		if (!['Physical', 'Special', 'Status'].includes(m.category)) fail(`${w}: category`);

		const multihit = m.multihit === undefined ? 'None'
			: Array.isArray(m.multihit) ? `Some((${int(m.multihit[0], 1, 255, w)}, ${int(m.multihit[1], 1, 255, w)}))`
				: `Some((${int(m.multihit, 1, 255, w)}, ${int(m.multihit, 1, 255, w)}))`;
		const damage = m.damage === undefined ? 'None' : m.damage === 'level' ? 'Some(FixedDamage::Level)'
			: `Some(FixedDamage::Hp(${int(m.damage, 1, 65535, w)}))`;
		const ohko = m.ohko === undefined ? 'Ohko::No' : m.ohko === true ? 'Ohko::Any' : `Ohko::Typed(${type(m.ohko, w)})`;
		const ignoreImmunity = m.ignoreImmunity === true ? 'IgnoreImmunity::All' : m.ignoreImmunity === false ? 'IgnoreImmunity::No'
			: Object.keys(m.ignoreImmunity).length === 1 && Object.values(m.ignoreImmunity)[0] === true
				? `IgnoreImmunity::Type(${type(Object.keys(m.ignoreImmunity)[0], w)})` : fail(`${w}: ignoreImmunity`);
		const selfSwitch = {undefined: 'SelfSwitch::No', true: 'SelfSwitch::Yes', copyvolatile: 'SelfSwitch::CopyVolatile',
			shedtail: 'SelfSwitch::ShedTail'}[m.selfSwitch] ?? fail(`${w}: selfSwitch ${m.selfSwitch}`);
		const selfdestruct = {undefined: 'SelfDestruct::No', always: 'SelfDestruct::Always', ifHit: 'SelfDestruct::IfHit'}[m.selfdestruct]
			?? fail(`${w}: selfdestruct ${m.selfdestruct}`);
		if (m.overrideOffensivePokemon !== undefined && m.overrideOffensivePokemon !== 'target') fail(`${w}: overrideOffensivePokemon`);
		if (m.nonGhostTarget && m.nonGhostTarget !== '' && !TARGETS.includes(m.nonGhostTarget)) fail(`${w}: nonGhostTarget`);

		let self = 'None';
		if (m.self) {
			checkKeys(m.self, new Set(['chance', 'boosts', 'volatileStatus', 'sideCondition', 'pseudoWeather']), new Set(), `${w}.self`);
			self = `Some(SelfEffect { chance: ${int(m.self.chance ?? 100, 1, 100, w)}, boosts: ${boosts(m.self.boosts, w)}, ` +
				`volatile_status: ${condition(m.self.volatileStatus)}, side_condition: ${condition(m.self.sideCondition)}, ` +
				`pseudo_weather: ${condition(m.self.pseudoWeather)} })`;
		}
		if (m.selfBoost) checkKeys(m.selfBoost, new Set(['boosts']), new Set(), `${w}.selfBoost`);

		const cond = m.condition ?? {};
		checkKeys(cond, new Set(['duration', 'counterMax', 'onLockMove', 'onInvulnerability', 'onCriticalHit']), new Set(), `${w}.condition`);
		if (cond.onLockMove !== undefined && cond.onLockMove !== id) fail(`${w}: condition locks into another move`);

		const z = m.zMove ?? {};
		checkKeys(z, new Set(['basePower', 'boost', 'effect']), new Set(), `${w}.zMove`);
		checkKeys(m.maxMove ?? {}, new Set(['basePower']), new Set(), `${w}.maxMove`);

		const fields = [
			`id: ${str(id)}`, `name: ${str(m.name)}`, `num: ${int(m.num, -32768, 32767, w)}`,
			`nonstandard: ${nonstandard(m.isNonstandard, w)}`,
			`move_type: ${type(m.type, w)}`, `category: MoveCategory::${m.category}`,
			`base_power: ${int(m.basePower, 0, 255, w)}`,
			`accuracy: ${m.accuracy === true ? 'None' : `Some(${int(m.accuracy, 1, 100, w)})`}`,
			`pp: ${int(m.pp, 1, 64, w)}`, `priority: ${int(m.priority, -8, 8, w)}`,
			`target: ${target(m.target, w)}`,
			`non_ghost_target: ${m.nonGhostTarget ? `Some(${target(m.nonGhostTarget, w)})` : 'None'}`,
			`flags: ${flagsExpr(m.flags, MOVE_FLAGS, 'MoveFlags', w)}`,
			`crit_ratio: ${int(m.critRatio, 0, 8, w)}`, `multihit: ${multihit}`,
			`drain: ${fraction(m.drain, w)}`, `recoil: ${fraction(m.recoil, w)}`, `heal: ${fraction(m.heal, w)}`,
			`fixed_damage: ${damage}`, `ohko: ${ohko}`, `ignore_immunity: ${ignoreImmunity}`,
			`status: ${status(m.status, w)}`,
			`volatile_status: ${condition(m.volatileStatus)}`, `side_condition: ${condition(m.sideCondition)}`,
			`slot_condition: ${condition(m.slotCondition)}`, `pseudo_weather: ${condition(m.pseudoWeather)}`,
			`weather: ${condition(m.weather)}`, `terrain: ${condition(m.terrain)}`,
			`boosts: ${boosts(m.boosts, w)}`, `self_effect: ${self}`, `self_boost: ${boosts(m.selfBoost?.boosts, w)}`,
			`secondaries: ${list((m.secondaries ?? []).map((s, i) => secondaryExpr(s, `${w}.secondaries[${i}]`)))}`,
			`condition_duration: ${int(cond.duration ?? 0, 0, 255, w)}`,
			`condition_counter_max: ${int(cond.counterMax ?? 0, 0, 65535, w)}`,
			`condition_locks_move: ${bool(cond.onLockMove !== undefined)}`,
			`condition_no_invulnerability: ${bool(constFalse(cond.onInvulnerability, w))}`,
			`condition_blocks_crits: ${bool(constFalse(cond.onCriticalHit, w))}`,
			`self_switch: ${selfSwitch}`, `selfdestruct: ${selfdestruct}`,
			`override_offensive_pokemon_target: ${bool(m.overrideOffensivePokemon === 'target')}`,
			`override_offensive_stat: ${m.overrideOffensiveStat ? `Some(${stat(m.overrideOffensiveStat, w)})` : 'None'}`,
			`override_defensive_stat: ${m.overrideDefensiveStat ? `Some(${stat(m.overrideDefensiveStat, w)})` : 'None'}`,
			`z_move: ${m.zMove ? `ZMoveData { base_power: ${int(z.basePower ?? 0, 0, 255, w)}, boosts: ${boosts(z.boost, w)}, effect: ${str(z.effect ?? '')} }` : 'ZMoveData::NONE'}`,
			`max_move_power: ${int(m.maxMove?.basePower ?? 0, 0, 255, w)}`,
			...Object.entries(MOVE_BOOL).map(([k, f]) => `${f}: ${bool(m[k])}`),
			`event_orders: ${list([...eventOrders(m), ...eventOrders(cond, 'condition.')])}`,
			`handlers: ${list((m.handlers ?? []).map(str))}`,
		];
		out.push(record('MoveData', fields));
	}
	out.push('];');
	out.push('');
	return out;
}

// ---------------------------------------------------------------------------------------
// Items

const ITEM_DROPPED = new Set(['affectsFainted', 'spritenum', 'tags']);
const ITEM_HANDLED = new Set(['name', 'id', 'num', 'isNonstandard', 'fling', 'isBerry', 'isGem', 'isChoice',
	'isPokeball', 'isPrimalOrb', 'ignoreKlutz', 'onTakeItem', 'onEat', 'onNegateImmunity', 'onFractionalPriority',
	'megaStone', 'itemUser', 'forcedForme', 'onPlate', 'onMemory', 'onDrive', 'zMove', 'zMoveFrom', 'zMoveType',
	'boosts', 'naturalGift', 'condition', 'handlers']);

// A constant `false` in place of a callback: the event is suppressed for this item/ability.
function constFalse(v, what) {
	if (v === undefined) return false;
	if (v === false) return true;
	fail(`${what}: expected false, got ${JSON.stringify(v)}`);
}

function fractionalPriority(v, what) {
	if (v === undefined) return '0';
	return int(Math.round(v * 10), -10, 10, what);
}

function genItems() {
	const t = tables.items;
	const out = [];
	out.push(`pub static ITEMS: [ItemData; ${t.sorted.length + 1}] = [`);
	out.push('    ItemData::NONE,');
	for (const id of t.sorted) {
		const it = t.entries[id];
		const w = `item ${id}`;
		checkKeys(it, ITEM_HANDLED, ITEM_DROPPED, w);
		let fling = 'None';
		if (it.fling) {
			checkKeys(it.fling, new Set(['basePower', 'status', 'volatileStatus']), new Set(), `${w}.fling`);
			fling = `Some(Fling { base_power: ${int(it.fling.basePower, 0, 255, w)}, status: ${status(it.fling.status, w)}, ` +
				`volatile_status: ${condition(it.fling.volatileStatus)} })`;
		}
		const mega = Object.entries(it.megaStone ?? {}).map(([from, to]) => `(${ref(tables.species, from, w)}, ${ref(tables.species, to, w)})`);
		const cond = it.condition ?? {};
		checkKeys(cond, new Set(['duration']), new Set(), `${w}.condition`);
		let zMove = 'None';
		if (it.zMove !== undefined) {
			zMove = `Some(ZCrystal { move_id: ${it.zMove === true ? 'MoveId::NONE' : ref(tables.moves, it.zMove, w)}, ` +
				`from: ${ref(tables.moves, it.zMoveFrom, w)}, move_type: ${type(it.zMoveType, w)} })`;
		} else if (it.zMoveFrom || it.zMoveType) {
			fail(`${w}: zMoveFrom/zMoveType without zMove`);
		}
		let naturalGift = 'None';
		if (it.naturalGift) {
			checkKeys(it.naturalGift, new Set(['basePower', 'type']), new Set(), `${w}.naturalGift`);
			naturalGift = `Some((${int(it.naturalGift.basePower, 1, 255, w)}, ${type(it.naturalGift.type, w)}))`;
		}
		out.push(record('ItemData', [
			`id: ${str(id)}`, `name: ${str(it.name)}`, `num: ${int(it.num, -32768, 32767, w)}`,
			`nonstandard: ${nonstandard(it.isNonstandard, w)}`, `fling: ${fling}`,
			`is_berry: ${bool(it.isBerry)}`, `is_gem: ${bool(it.isGem)}`, `is_choice: ${bool(it.isChoice)}`,
			`is_pokeball: ${bool(it.isPokeball)}`, `is_primal_orb: ${bool(it.isPrimalOrb)}`,
			`ignore_klutz: ${bool(it.ignoreKlutz)}`,
			`cannot_be_taken: ${bool(constFalse(it.onTakeItem, w))}`, `no_eat_effect: ${bool(constFalse(it.onEat, w))}`,
			`no_negate_immunity: ${bool(constFalse(it.onNegateImmunity, w))}`,
			`fractional_priority_tenths: ${fractionalPriority(it.onFractionalPriority, w)}`,
			`mega_stone: ${list(mega)}`,
			`item_users: ${list((it.itemUser ?? []).map(n => ref(tables.species, n, w)))}`,
			`forced_forme: ${ref(tables.species, it.forcedForme, w)}`,
			`plate_type: ${type(it.onPlate, w)}`, `memory_type: ${type(it.onMemory, w)}`, `drive_type: ${type(it.onDrive, w)}`,
			`z_crystal: ${zMove}`, `boosts: ${boosts(it.boosts, w)}`, `natural_gift: ${naturalGift}`,
			`condition_duration: ${int(cond.duration ?? 0, 0, 255, w)}`,
			`event_orders: ${list([...eventOrders(it), ...eventOrders(cond, 'condition.')])}`,
			`handlers: ${list((it.handlers ?? []).map(str))}`,
		]));
	}
	out.push('];');
	out.push('');
	return out;
}

// ---------------------------------------------------------------------------------------
// Abilities

const ABILITY_FLAGS = [...new Set(Object.values(data.abilities).flatMap(a => Object.keys(a.flags)))].sort();
if (ABILITY_FLAGS.length > 16) fail('more than 16 ability flags');

function genAbilities() {
	const t = tables.abilities;
	const out = [];
	out.push(`pub static ABILITIES: [AbilityData; ${t.sorted.length + 1}] = [`);
	out.push('    AbilityData::NONE,');
	const handled = new Set(['name', 'id', 'num', 'isNonstandard', 'flags', 'suppressWeather', 'onCriticalHit',
		'onFractionalPriority', 'condition', 'handlers']);
	for (const id of t.sorted) {
		const a = t.entries[id];
		const w = `ability ${id}`;
		checkKeys(a, handled, new Set(['affectsFainted']), w);
		checkKeys(a.condition ?? {}, new Set(), new Set(), `${w}.condition`);
		out.push(record('AbilityData', [
			`id: ${str(id)}`, `name: ${str(a.name)}`, `num: ${int(a.num, -32768, 32767, w)}`,
			`nonstandard: ${nonstandard(a.isNonstandard, w)}`,
			`flags: ${flagsExpr(a.flags, ABILITY_FLAGS, 'AbilityFlags', w)}`,
			`suppress_weather: ${bool(a.suppressWeather)}`,
			`cannot_be_crit: ${bool(constFalse(a.onCriticalHit, w))}`,
			`fractional_priority_tenths: ${fractionalPriority(a.onFractionalPriority, w)}`,
			`event_orders: ${list([...eventOrders(a), ...eventOrders(a.condition ?? {}, 'condition.')])}`,
			`handlers: ${list((a.handlers ?? []).map(str))}`,
		]));
	}
	out.push('];');
	out.push('');
	return out;
}

// ---------------------------------------------------------------------------------------
// Id constants and flag bits

function genConstants(table) {
	const out = [];
	const ty = idType[table.kind];
	out.push(`pub mod ${table.kind} {`);
	out.push(`    use super::${ty};`);
	for (const [c, id] of [...table.names].sort((a, b) => table.index.get(a[1]) - table.index.get(b[1]))) {
		out.push(`    pub const ${c}: ${ty} = ${ty}(${table.index.get(id)});`);
	}
	out.push('}');
	out.push('');
	return out;
}

function genFlagConsts(typeName, flags) {
	const out = [`impl ${typeName} {`];
	flags.forEach((f, i) => out.push(`    pub const ${f.toUpperCase()}: ${typeName} = ${typeName}(1 << ${i});`));
	out.push('}');
	out.push('');
	return out;
}

// ---------------------------------------------------------------------------------------

function generate() {
	const out = [];
	out.push('// @generated by engine/data/gen-rust.cjs from engine/data/champions.json. Do not edit.');
	out.push(`// Source: ${data.source.repo}@${data.source.commit} (mod ${data.source.mod}).`);
	out.push('');
	out.push('use super::*;');
	out.push('use crate::state::Status;');
	out.push('');
	out.push(`pub const SOURCE_COMMIT: &str = ${str(data.source.commit)};`);
	out.push('');
	out.push('/// Exported species left out of the tables, with the reason.');
	out.push(`pub const EXCLUDED_SPECIES: &[(&str, &str)] = ${list(Object.entries(EXCLUDED_SPECIES).map(([id, why]) => `(${str(id)}, ${str(why)})`))};`);
	out.push('');
	out.push(...genNoneConsts());
	out.push(...genTypes());
	out.push(...genNatures());
	out.push(...genFlagConsts('MoveFlags', MOVE_FLAGS));
	out.push(...genFlagConsts('AbilityFlags', ABILITY_FLAGS));
	out.push(...genConditions());
	for (const t of Object.values(tables)) out.push(...genConstants(t));
	out.push(...genSpecies());
	out.push(...genMoves());
	out.push(...genItems());
	out.push(...genAbilities());
	return out.join('\n');
}

function main() {
	const text = generate();
	if (process.argv.includes('--check')) {
		const current = fs.existsSync(OUT) ? fs.readFileSync(OUT, 'utf8') : '';
		if (current.replace(/\r\n/g, '\n') !== text) {
			console.error(`${path.relative(process.cwd(), OUT)} is out of date; run node engine/data/gen-rust.cjs`);
			process.exit(1);
		}
		console.log('generated dex is up to date');
		return;
	}
	fs.mkdirSync(path.dirname(OUT), {recursive: true});
	fs.writeFileSync(OUT, text);
	const n = k => tables[k].sorted.length;
	console.log(`wrote ${path.relative(process.cwd(), OUT)}: types ${TYPES.length}, natures ${Object.keys(data.natures).length}, ` +
		`species ${n('species')}, moves ${n('moves')}, items ${n('items')}, abilities ${n('abilities')}, conditions ${CONDITIONS.length}`);
}

main();
