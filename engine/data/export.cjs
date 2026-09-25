'use strict';
// Exports the Champions dex from vendor/pokemon-showdown (mod `champions`) for lab-engine.
//
// Scope rule (2026-09-25): mechanics follow Champions, but the roster is not limited to the
// current regulation. Every species, move, item and ability the Champions mod can load is
// exported with its `isNonstandard` tag kept as data; legality is the validator's job.
//
// Plain data fields are copied as-is. Behaviour that Showdown implements as callbacks
// (onHit, basePowerCallback, ...) cannot be exported, so each entry lists its callback names
// under `handlers`: those are what lab-engine has to implement by hand, and the list doubles
// as the coverage checklist.
//
// Usage: node engine/data/export.cjs [--out engine/data/champions.json]

const fs = require('node:fs');
const path = require('node:path');
const root = path.resolve(__dirname, '../..');
const {Dex} = require(path.join(root, 'vendor/pokemon-showdown/dist/sim'));

const dex = Dex.mod('champions');

// Showdown object fields that are bookkeeping, not game data.
const SKIP = new Set(['effectType', 'exists', 'fullname', 'kind', 'gen', 'sourceEffect', 'desc', 'shortDesc',
	'rating', 'tier', 'doublesTier', 'natDexTier', 'spriteid', 'realMove', 'baseMoveType', 'noCopy']);

function plain(value, handlers, prefix = '') {
	if (typeof value === 'function') {
		handlers.push(prefix);
		return undefined;
	}
	if (Array.isArray(value)) return value.map(v => plain(v, handlers, prefix));
	if (value && typeof value === 'object') {
		const out = {};
		for (const [k, v] of Object.entries(value)) {
			if (SKIP.has(k) || v === undefined) continue;
			const p = plain(v, handlers, prefix ? `${prefix}.${k}` : k);
			if (p !== undefined) out[k] = p;
		}
		return out;
	}
	return value;
}

function entry(obj) {
	const handlers = [];
	const data = plain({...obj}, handlers);
	if (handlers.length) data.handlers = handlers.sort();
	return data;
}

function table(list) {
	const out = {};
	for (const obj of list) {
		if (!obj.exists) continue;
		out[obj.id] = entry(obj);
	}
	return out;
}

function main() {
	const outArg = process.argv.indexOf('--out');
	const out = outArg >= 0 ? process.argv[outArg + 1] : path.join(__dirname, 'champions.json');
	const data = {
		source: {
			repo: 'smogon/pokemon-showdown',
			commit: commit(),
			mod: 'champions',
			exportedBy: 'engine/data/export.cjs',
		},
		types: Object.fromEntries(dex.types.all().filter(t => t.exists).map(t => [t.id, {
			name: t.name,
			// damageTaken[attackingType]: 0 normal, 1 super effective, 2 resisted, 3 immune
			damageTaken: t.damageTaken,
			isNonstandard: t.isNonstandard ?? null,
		}])),
		natures: Object.fromEntries(dex.natures.all().map(n => [n.id, {name: n.name, plus: n.plus ?? null, minus: n.minus ?? null}])),
		species: table(dex.species.all()),
		moves: table(dex.moves.all()),
		items: table(dex.items.all()),
		abilities: table(dex.abilities.all()),
		conditions: conditions(),
	};
	fs.writeFileSync(out, JSON.stringify(data, null, 1) + '\n');
	const count = k => Object.keys(data[k]).length;
	const nonstandard = k => Object.values(data[k]).filter(v => v.isNonstandard).length;
	console.log(`wrote ${path.relative(root, out)}: ` + ['species', 'moves', 'items', 'abilities']
		.map(k => `${k} ${count(k)} (${nonstandard(k)} nonstandard)`).join(', '));
}

// Status, weather and callback-created volatile conditions (slp, sandstorm, stall, ...). Their behaviour is all
// callbacks, but durations and the handler list are still useful.
function conditions() {
	const ids = ['brn', 'par', 'slp', 'frz', 'psn', 'tox', 'confusion', 'flinch', 'partiallytrapped',
		'sunnyday', 'raindance', 'sandstorm', 'snowscape', 'desolateland', 'primordialsea', 'deltastream',
		// Volatiles that moves create in callbacks, not through a `volatileStatus` field.
		'stall', 'choicelock', 'lockedmove', 'twoturnmove'];
	const out = {};
	for (const id of ids) {
		const c = dex.conditions.get(id);
		if (c.exists) out[id] = entry(c);
	}
	return out;
}

function commit() {
	const git = path.join(root, 'vendor/pokemon-showdown/.git');
	try {
		const head = fs.readFileSync(path.join(git, 'HEAD'), 'utf8').trim();
		return head.startsWith('ref:') ? fs.readFileSync(path.join(git, head.slice(5)), 'utf8').trim() : head;
	} catch {
		return null;
	}
}

main();
