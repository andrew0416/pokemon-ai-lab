'use strict';
// Canonical battle state: the comparison format shared by the Showdown oracle and lab-engine.
//
// Only state that can change what happens later is included; log text, effect order and
// source references are not. Pokémon are keyed by name (unique per side), because Showdown
// reorders side.pokemon on every switch. Bump SCHEMA when the shape changes.

const SCHEMA = 1;

// Effect-state fields that carry game state. Everything else in an EffectState is bookkeeping.
const EFFECT_FIELDS = ['duration', 'counter', 'layers', 'stage', 'time', 'startTime', 'hp', 'move', 'turns'];

function effect(state) {
	const out = {};
	for (const k of EFFECT_FIELDS) {
		const v = state[k];
		if (v === undefined || v === null) continue;
		out[k] = typeof v === 'object' ? v.id ?? String(v) : v;
	}
	return out;
}

function effects(table) {
	const out = {};
	for (const id of Object.keys(table || {}).sort()) out[id] = effect(table[id]);
	return out;
}

function boosts(b) {
	const out = {};
	for (const k of ['atk', 'def', 'spa', 'spd', 'spe', 'accuracy', 'evasion']) if (b[k]) out[k] = b[k];
	return out;
}

function pokemon(mon) {
	const out = {
		name: mon.name,
		species: mon.species.name,
		hp: mon.hp,
		maxhp: mon.maxhp,
		status: mon.status || '',
		item: mon.item || '',
		ability: mon.ability,
		slot: mon.isActive ? mon.position : null,
		pp: Object.fromEntries(mon.moveSlots.map(m => [m.id, m.pp])),
	};
	if (mon.status === 'slp') out.statusTime = mon.statusState.time;
	if (mon.status === 'tox') out.statusStage = mon.statusState.stage;
	if (mon.lastItem) out.lastItem = mon.lastItem;
	if (mon.canMegaEvo) out.canMega = true;
	if (mon.isActive) {
		out.boosts = boosts(mon.boosts);
		out.volatiles = effects(mon.volatiles);
		if (mon.lastMove) out.lastMove = mon.lastMove.id;
		const types = mon.getTypes(true).join('/');
		if (types !== mon.species.types.join('/')) out.types = types;
	}
	return out;
}

function side(s) {
	return {
		request: s.requestState || '',
		conditions: effects(s.sideConditions),
		slotConditions: s.slotConditions.map(effects),
		pokemon: [...s.pokemon].sort((a, b) => (a.name < b.name ? -1 : 1)).map(pokemon),
	};
}

function canonical(battle) {
	const f = battle.field;
	return {
		schema: SCHEMA,
		turn: battle.turn,
		ended: battle.ended,
		winner: battle.winner || '',
		field: {
			weather: f.weather || '',
			weatherDuration: f.weather ? f.weatherState.duration ?? null : null,
			terrain: f.terrain || '',
			terrainDuration: f.terrain ? f.terrainState.duration ?? null : null,
			pseudoWeather: effects(f.pseudoWeather),
		},
		sides: [side(battle.p1), side(battle.p2)],
	};
}

function canonicalKey(state) {
	return JSON.stringify(state);
}

module.exports = {SCHEMA, canonical, canonicalKey};
