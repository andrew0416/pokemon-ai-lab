'use strict';
// Showdown oracle: the exact outcome distribution of one turn.
//
// Every random call in Showdown goes through battle.prng (random / randomChance / sample /
// shuffle). This script swaps the PRNG for a scripted one and walks every branch depth-first,
// re-running the turn from a serialized snapshot for each branch. Identical end states are
// merged by their canonical form (canonical.cjs), which lab-engine must reproduce exactly.
//
// Usage:
//   node engine/oracle/enumerate.cjs <scenario.json> [--mode full|extremes|mc] [--samples N]
//                                    [--max-branches N] [--keep-nominal-draws] [--out file]
//
// Modes:
//   full      every branch with its exact probability (the parity reference)
//   extremes  damage rolls only take the min and max roll; probabilities are NOT exact
//   mc        N natural PRNG runs, empirical frequencies (cross-checks `full`)

const fs = require('node:fs');
const path = require('node:path');
// LAB_ROOT: git worktree 등 vendor/가 없는 체크아웃에서 본 저장소(vendor/pokemon-showdown 포함)를 가리킨다.
const root = process.env.LAB_ROOT ? path.resolve(process.env.LAB_ROOT) : path.resolve(__dirname, '../..');
const {Battle, PRNG, Teams} = require(path.join(root, 'vendor/pokemon-showdown/dist/sim'));
const {canonical, canonicalKey} = require('./canonical.cjs');

function parseArgs(argv) {
	const args = {mode: 'full', samples: 20000, maxBranches: 500000, out: null, file: null, keepNominalDraws: false};
	for (let i = 0; i < argv.length; i++) {
		const a = argv[i];
		if (a === '--mode') args.mode = argv[++i];
		else if (a === '--samples') args.samples = Number(argv[++i]);
		else if (a === '--max-branches') args.maxBranches = Number(argv[++i]);
		else if (a === '--out') args.out = argv[++i];
		else if (a === '--keep-nominal-draws') args.keepNominalDraws = true;
		else if (!args.file) args.file = a;
		else throw new Error(`unexpected argument ${a}`);
	}
	if (!args.file) throw new Error('usage: enumerate.cjs <scenario.json> [--mode full|extremes|mc]');
	if (!['full', 'extremes', 'mc'].includes(args.mode)) throw new Error(`unknown mode ${args.mode}`);
	return args;
}

function readJSON(file) {
	return JSON.parse(fs.readFileSync(file, 'utf8').replace(/^﻿/, ''));
}

// `adjustLevel`: the format's `Adjust Level` rule (VGC: 50). The team validator, which the
// oracle does not run, sets every valid set to that level; the battle itself does not.
function loadTeam(spec, baseDir, adjustLevel = null) {
	const team = typeof spec === 'string' ? readJSON(path.resolve(baseDir, spec)) : spec;
	const names = new Set();
	for (const set of team) {
		if (adjustLevel) set.level = adjustLevel;
		set.name = set.name || set.species;
		if (names.has(set.name)) throw new Error(`duplicate name ${set.name}: canonical states key Pokémon by name`);
		names.add(set.name);
	}
	return team;
}

// ---- scenario setup -------------------------------------------------------------------------

function findPokemon(side, name) {
	const mon = side.pokemon.find(p => p.name === name);
	if (!mon) throw new Error(`${side.id} has no Pokémon named ${name}`);
	return mon;
}

function applyPatch(battle, patch) {
	if (!patch) return;
	for (const sideId of ['p1', 'p2']) {
		const side = battle[sideId];
		for (const [name, p] of Object.entries(patch[sideId] || {})) {
			const mon = findPokemon(side, name);
			if (p.hp !== undefined) mon.sethp(p.hp);
			if (p.status !== undefined) {
				mon.clearStatus();
				if (p.status) {
					mon.setStatus(p.status);
					if (p.statusTime !== undefined) {
						mon.statusState.time = p.statusTime;
						mon.statusState.startTime = p.statusTime;
					}
				}
			}
			if (p.boosts) {
				if (!mon.isActive) throw new Error(`boosts on inactive ${name}`);
				Object.assign(mon.boosts, p.boosts);
			}
			if (p.item !== undefined) mon.item = p.item;
		}
		for (const [id, duration] of Object.entries(patch.sides?.[sideId] || {})) {
			side.addSideCondition(id, side.active[0]);
			if (duration !== null) side.sideConditions[id].duration = duration;
		}
	}
	// Field effects need a source Pokémon; it only matters for logs and source-based durations,
	// which the patch overrides anyway.
	const f = patch.field || {};
	const source = battle.p1.active[0];
	if (f.weather) {
		battle.field.setWeather(f.weather, source);
		if (f.weatherDuration !== undefined) battle.field.weatherState.duration = f.weatherDuration;
	}
	if (f.terrain) {
		battle.field.setTerrain(f.terrain, source);
		if (f.terrainDuration !== undefined) battle.field.terrainState.duration = f.terrainDuration;
	}
	for (const [id, duration] of Object.entries(f.pseudoWeather || {})) {
		battle.field.addPseudoWeather(id, source);
		if (duration !== null) battle.field.pseudoWeather[id].duration = duration;
	}
}

// Builds the battle up to the decision point and returns its serialized snapshot.
function buildSnapshot(scenario, baseDir) {
	const battle = new Battle({
		formatid: scenario.format,
		seed: scenario.seed || 'sodium,00000000000000000000000000000000',
		strictChoices: true,
	});
	const adjustLevel = battle.ruleTable.adjustLevel;
	battle.setPlayer('p1', {name: 'p1', team: Teams.pack(loadTeam(scenario.p1.team, baseDir, adjustLevel))});
	battle.setPlayer('p2', {name: 'p2', team: Teams.pack(loadTeam(scenario.p2.team, baseDir, adjustLevel))});
	if (battle.requestState === 'teampreview') {
		battle.makeChoices(`team ${scenario.p1.order || '123456'}`, `team ${scenario.p2.order || '123456'}`);
	}
	for (const [c1, c2, midTurn] of scenario.setupTurns || []) {
		battle.makeChoices(c1, c2);
		applyMidTurn(battle, midTurn);
	}
	applyPatch(battle, scenario.patch);
	if (battle.ended) throw new Error('battle ended during setup');
	return {snapshot: JSON.stringify(battle.toJSON()), before: canonical(battle)};
}

// ---- scripted PRNG ---------------------------------------------------------------------------

class ExhaustedTrace extends Error {}

// Replays `prefix` (one chosen index per decision point), then takes index 0 for every new
// decision point, recording each point's weights so the caller can advance the odometer.
class ScriptedPRNG {
	constructor(prefix, mode) {
		this.prefix = prefix;
		this.mode = mode;
		this.trace = []; // {weights, choice}
		this.approximate = false;
		this.nextIsRoll = false;
	}
	decide(values, weights) {
		const i = this.trace.length;
		const choice = i < this.prefix.length ? this.prefix[i] : 0;
		if (choice >= values.length) throw new ExhaustedTrace(`prefix diverged at decision ${i}`);
		this.trace.push({weights, choice});
		return values[choice];
	}
	random(from, to) {
		if (from === undefined) throw new Error('float random() is not enumerable');
		from = Math.floor(from);
		if (to) to = Math.floor(to);
		const lo = to ? from : 0;
		const hi = to ? to : from;
		const n = hi - lo;
		if (n <= 1) return lo;
		if (this.nextIsRoll) {
			this.nextIsRoll = false;
			if (this.mode === 'extremes' && n === 16) {
				this.approximate = true;
				return this.decide([0, 15], [0.5, 0.5]);
			}
		}
		const values = Array.from({length: n}, (_, k) => lo + k);
		return this.decide(values, values.map(() => 1 / n));
	}
	// Showdown's PRNG: `this.random(denominator) < numerator`, an integer draw in [0, denominator),
	// so a non-integer numerator (a multi-accuracy re-roll's float accuracy, e.g. 67.5) succeeds
	// on ceil(numerator) of the denominator values. Integer numerators are unchanged.
	randomChance(numerator, denominator) {
		const hits = Math.ceil(numerator);
		if (hits >= denominator) return true;
		if (hits <= 0) return false;
		const p = hits / denominator;
		return this.decide([true, false], [p, 1 - p]);
	}
	sample(items) {
		if (!items.length) throw new RangeError('Cannot sample an empty array');
		return items[this.random(items.length)];
	}
	shuffle(items, start = 0, end = items.length) {
		while (start < end - 1) {
			const j = this.random(start, end);
			if (start !== j) [items[start], items[j]] = [items[j], items[start]];
			start++;
		}
	}
	getSeed() {
		return 'scripted';
	}
	clone() {
		throw new Error('ScriptedPRNG cannot be cloned');
	}
}

function restore(snapshot) {
	return Battle.fromJSON(snapshot);
}

function runTurn(battle, scenario) {
	const logStart = battle.log.length;
	battle.makeChoices(scenario.turn.p1, scenario.turn.p2);
	applyMidTurn(battle, scenario.midTurn);
	return {state: canonical(battle), log: turnLog(battle.log.slice(logStart))};
}

// A mid-turn switch request (U-turn, Parting Shot, Eject Button, Emergency Exit, ...) pauses
// the battle for a living Pokémon that must switch out (`forceSwitch` on an active with HP),
// possibly after the residual phase. Every requesting side gets its next `midTurn` choice
// (`{p1: ["switch 3"], p2: []}`); if one has none left the battle stays paused there and the
// paused state (its `request` is `switch`) is the outcome. An end-of-turn replacement request
// (for fainted Pokémon only) is the next decision, not handled here.
function midTurnRequest(side) {
	if (side.requestState !== 'switch') return false;
	return side.active.some((pokemon, i) => pokemon && pokemon.hp > 0 && side.activeRequest.forceSwitch[i]);
}

function applyMidTurn(battle, midTurn) {
	const used = {p1: 0, p2: 0};
	while (battle.sides.some(midTurnRequest)) {
		const requesting = battle.sides.filter(midTurnRequest);
		const choices = requesting.map(side => ((midTurn && midTurn[side.id]) || [])[used[side.id]]);
		if (!requesting.length || choices.some(choice => choice === undefined)) return;
		requesting.forEach((side, i) => {
			used[side.id]++;
			if (!battle.choose(side.id, choices[i])) {
				throw new Error(`${side.id}: mid-turn choice "${choices[i]}" rejected: ${battle.log.slice(-1)}`);
			}
		});
	}
}

// Showdown picks a random foe as the nominal target of a doubles spread move (Rock Slide,
// Earthquake, ...) while queueing it. The pick consumes RNG but the move hits every target
// anyway, so branching on it only multiplies identical outcomes. Unless --keep-nominal-draws
// is given, the oracle takes the first living foe instead. `mc` mode never collapses, so
// comparing `full` against `mc` checks that this is outcome-neutral.
const NOMINAL_TARGETS = new Set(['allAdjacentFoes', 'allAdjacent', 'foeSide']);

function collapseNominalTargets(battle) {
	const getRandomTarget = battle.getRandomTarget;
	battle.getRandomTarget = function (pokemon, move) {
		const m = this.dex.moves.get(move);
		if (this.gameType === 'doubles' && NOMINAL_TARGETS.has(m.target)) {
			return pokemon.side.foe.active.find(p => p && !p.fainted) || pokemon.side.foe.active[0];
		}
		return getRandomTarget.call(this, pokemon, move);
	};
}

// Damage rolls are the one random call worth tagging: `extremes` mode collapses them.
function tagDamageRolls(battle, prng) {
	const randomizer = battle.randomizer;
	battle.randomizer = function (baseDamage) {
		prng.nextIsRoll = true;
		return randomizer.call(this, baseDamage);
	};
}

function nextPrefix(trace) {
	for (let i = trace.length - 1; i >= 0; i--) {
		if (trace[i].choice + 1 < trace[i].weights.length) {
			return [...trace.slice(0, i).map(t => t.choice), trace[i].choice + 1];
		}
	}
	return null;
}

function enumerate(scenario, snapshot, mode, maxBranches, keepNominalDraws = false) {
	const outcomes = new Map();
	let prefix = [];
	let branches = 0;
	let approximate = false;
	let maxDepth = 0;
	while (prefix) {
		if (++branches > maxBranches) throw new Error(`more than ${maxBranches} branches; use --mode extremes or mc`);
		const battle = restore(snapshot);
		const prng = new ScriptedPRNG(prefix, mode);
		battle.prng = prng;
		tagDamageRolls(battle, prng);
		if (!keepNominalDraws) collapseNominalTargets(battle);
		const {state, log} = runTurn(battle, scenario);
		const p = prng.trace.reduce((acc, t) => acc * t.weights[t.choice], 1);
		approximate ||= prng.approximate;
		maxDepth = Math.max(maxDepth, prng.trace.length);
		const key = canonicalKey(state);
		const entry = outcomes.get(key);
		if (entry) {
			entry.p += p;
			entry.branches++;
		} else {
			outcomes.set(key, {p, branches: 1, state, log});
		}
		prefix = nextPrefix(prng.trace);
	}
	return {branches, maxDepth, approximate, outcomes};
}

function monteCarlo(scenario, snapshot, samples) {
	const outcomes = new Map();
	for (let i = 0; i < samples; i++) {
		const battle = restore(snapshot);
		battle.prng = new PRNG(`sodium,${i.toString(16).padStart(64, '0')}`);
		const {state, log} = runTurn(battle, scenario);
		const key = canonicalKey(state);
		const entry = outcomes.get(key);
		if (entry) {
			entry.p += 1 / samples;
			entry.branches++;
		} else {
			outcomes.set(key, {p: 1 / samples, branches: 1, state, log});
		}
	}
	return {branches: samples, maxDepth: null, approximate: true, outcomes};
}

// The turn's own log lines, for reading an outcome by eye.
// After `|split|<side>` Showdown writes the exact-HP line for that side, then the public one;
// keep the exact line.
function turnLog(lines) {
	const out = [];
	for (let i = 0; i < lines.length; i++) {
		const l = lines[i];
		if (l.startsWith('|split|')) {
			out.push(lines[i + 1]);
			i += 2;
		} else if (l !== '|' && !l.startsWith('|t:|') && !l.startsWith('|debug|')) {
			out.push(l);
		}
	}
	return out;
}

function main() {
	const args = parseArgs(process.argv.slice(2));
	const scenarioPath = path.resolve(args.file);
	const scenario = readJSON(scenarioPath);
	const baseDir = path.dirname(scenarioPath);
	const started = process.hrtime.bigint();
	const {snapshot, before} = buildSnapshot(scenario, baseDir);
	const result = args.mode === 'mc' ?
		monteCarlo(scenario, snapshot, args.samples) :
		enumerate(scenario, snapshot, args.mode, args.maxBranches, args.keepNominalDraws);
	const elapsedMs = Number(process.hrtime.bigint() - started) / 1e6;
	const outcomes = [...result.outcomes.values()]
		.sort((a, b) => b.p - a.p)
		.map(o => ({p: o.p, branches: o.branches, state: o.state, log: o.log}));
	const total = outcomes.reduce((s, o) => s + o.p, 0);
	const report = {
		scenario: path.relative(root, scenarioPath).replaceAll('\\', '/'),
		format: scenario.format,
		turn: scenario.turn,
		mode: args.mode,
		nominalTargetDraws: args.mode === 'mc' || args.keepNominalDraws ? 'kept' : 'collapsed',
		exact: args.mode === 'full' && !result.approximate,
		showdownCommit: sourceCommit(),
		branches: result.branches,
		maxDecisionDepth: result.maxDepth,
		distinctOutcomes: outcomes.length,
		totalProbability: total,
		elapsedMs: Math.round(elapsedMs),
		before,
		outcomes,
	};
	const text = JSON.stringify(report, null, 1);
	if (args.out) fs.writeFileSync(args.out, text);
	else process.stdout.write(text + '\n');
	console.error(`${args.mode}: ${result.branches} branches -> ${outcomes.length} outcomes, ` +
		`total p=${total.toFixed(12)}, ${Math.round(elapsedMs)} ms`);
}

function sourceCommit() {
	try {
		const head = fs.readFileSync(path.join(root, 'vendor/pokemon-showdown/.git/HEAD'), 'utf8').trim();
		if (!head.startsWith('ref:')) return head;
		return fs.readFileSync(path.join(root, 'vendor/pokemon-showdown/.git', head.slice(5)), 'utf8').trim();
	} catch {
		return null;
	}
}

module.exports = {buildSnapshot, enumerate, monteCarlo, ScriptedPRNG, nextPrefix, loadTeam, readJSON, sourceCommit};
if (require.main === module) main();
