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
//                                    [--collapse-secondaries] [--traces]
//                                    [--setup-walks N] [--setup-max-branches N]
//
// Modes:
//   full      every branch with its exact probability (the parity reference)
//   extremes  damage rolls only take the min and max roll; probabilities are NOT exact
//   mc        N natural PRNG runs, empirical frequencies (cross-checks `full`)
//
// --collapse-secondaries: a secondary effect's or self-drop's `random(100) < chance` draw becomes
// one two-way decision (exact; see collapsedActions). --traces: every outcome carries the
// scripted-PRNG trace of its first branch, which replays it (`setupTraces` below).
//
// Pinned positions (FF-parity-harness): `startState` (canonical state after the leads' switch-ins)
// and `setupStates[k]` (canonical state after setup turn k) keep only the branch of the start or of
// that setup turn that ends in the pinned state, instead of the scenario seed's natural path. The
// branch is found by replaying `startTrace`/`setupTraces[k]` (a trace from an earlier report) if
// given and still valid, else by up to --setup-walks random walks, else by a depth-first
// enumeration of up to --setup-max-branches branches (extremes mode). The report's `startTrace`
// and `setupTraces` are the branches used.

const fs = require('node:fs');
const path = require('node:path');
// LAB_ROOT: git worktree 등 vendor/가 없는 체크아웃에서 본 저장소(vendor/pokemon-showdown 포함)를 가리킨다.
const root = process.env.LAB_ROOT ? path.resolve(process.env.LAB_ROOT) : path.resolve(__dirname, '../..');
const {Battle, PRNG, Teams} = require(path.join(root, 'vendor/pokemon-showdown/dist/sim'));
const {canonical, canonicalKey} = require('./canonical.cjs');

function parseArgs(argv) {
	const args = {
		mode: 'full', samples: 20000, maxBranches: 500000, out: null, file: null, keepNominalDraws: false,
		collapse: false, traces: false, setupWalks: 2000, setupMaxBranches: 200000,
	};
	for (let i = 0; i < argv.length; i++) {
		const a = argv[i];
		if (a === '--mode') args.mode = argv[++i];
		else if (a === '--samples') args.samples = Number(argv[++i]);
		else if (a === '--max-branches') args.maxBranches = Number(argv[++i]);
		else if (a === '--out') args.out = argv[++i];
		else if (a === '--keep-nominal-draws') args.keepNominalDraws = true;
		else if (a === '--collapse-secondaries') args.collapse = true;
		else if (a === '--traces') args.traces = true;
		else if (a === '--setup-walks') args.setupWalks = Number(argv[++i]);
		else if (a === '--setup-max-branches') args.setupMaxBranches = Number(argv[++i]);
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

const DEFAULT_SEED = 'sodium,00000000000000000000000000000000';

function newBattle(scenario, baseDir) {
	const battle = new Battle({
		formatid: scenario.format,
		seed: scenario.seed || DEFAULT_SEED,
		strictChoices: true,
	});
	const adjustLevel = battle.ruleTable.adjustLevel;
	battle.setPlayer('p1', {name: 'p1', team: Teams.pack(loadTeam(scenario.p1.team, baseDir, adjustLevel))});
	battle.setPlayer('p2', {name: 'p2', team: Teams.pack(loadTeam(scenario.p2.team, baseDir, adjustLevel))});
	return battle;
}

function chooseTeams(battle, scenario) {
	if (battle.requestState === 'teampreview') {
		battle.makeChoices(`team ${scenario.p1.order || '123456'}`, `team ${scenario.p2.order || '123456'}`);
	}
}

// Builds the battle up to the decision point and returns its serialized snapshot. `opts` (the
// command-line options) only matter for pinned states: how their branches are searched.
function buildSnapshot(scenario, baseDir, opts = {}) {
	const traces = {start: null, setup: []};
	let battle;
	if (scenario.startState) {
		const found = findPinned(() => newBattle(scenario, baseDir), b => chooseTeams(b, scenario),
			scenario.startState, scenario.startTrace, {...opts, what: 'startState'});
		battle = found.battle;
		traces.start = found.trace;
	} else {
		battle = newBattle(scenario, baseDir);
		chooseTeams(battle, scenario);
	}
	const pins = scenario.setupStates || [];
	const known = scenario.setupTraces || [];
	for (const [k, [c1, c2, midTurn]] of (scenario.setupTurns || []).entries()) {
		if (pins[k]) {
			const snapshot = JSON.stringify(battle.toJSON());
			const found = findPinned(() => restore(snapshot), b => {
				b.makeChoices(c1, c2);
				applyMidTurn(b, midTurn);
			}, pins[k], known[k], {...opts, what: `setup turn ${k + 1}`});
			battle = found.battle;
			traces.setup.push(found.trace);
		} else {
			battle.makeChoices(c1, c2);
			applyMidTurn(battle, midTurn);
			traces.setup.push(null);
		}
	}
	applyPatch(battle, scenario.patch);
	if (battle.ended) throw new Error('battle ended during setup');
	return {snapshot: JSON.stringify(battle.toJSON()), before: canonical(battle), traces};
}

// ---- pinned branches -----------------------------------------------------------------------

// Canonical states compared regardless of key order (a pinned state may come from lab-engine).
function sortKeys(v) {
	if (Array.isArray(v)) return v.map(sortKeys);
	if (v && typeof v === 'object') {
		const out = {};
		for (const k of Object.keys(v).sort()) out[k] = sortKeys(v[k]);
		return out;
	}
	return v;
}

function stableKey(state) {
	return JSON.stringify(sortKeys(state));
}

// Deterministic uniform [0, 1) stream for the random walks.
function mulberry32(seed) {
	let a = seed >>> 0;
	return () => {
		a = (a + 0x6d2b79f5) >>> 0;
		let t = a;
		t = Math.imul(t ^ (t >>> 15), t | 1);
		t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
		return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
	};
}

// Finds a branch of `run(battle)` (one step played on a fresh battle from `make()`) that ends in
// the canonical state `pin`: the recorded `trace` if it still leads there, else random walks that
// draw every decision by its probability (quick for the likely outcomes a recorded game mostly
// has), else a depth-first enumeration in extremes mode (pins recorded under min/max rolls are
// reachable there). Returns that branch's battle, with a real PRNG again, and its trace.
function findPinned(make, run, pin, trace, opts) {
	const target = stableKey(pin);
	const settings = {collapse: !!opts.collapse, nominal: !opts.keepNominalDraws};
	const attempt = prng => {
		const battle = make();
		instrument(battle, prng, opts);
		run(battle);
		return battle;
	};
	const found = (battle, prng) => {
		battle.prng = new PRNG(DEFAULT_SEED);
		return {battle, trace: {mode: prng.mode, ...settings, prefix: prng.trace.map(t => t.choice)}};
	};
	if (trace && trace.collapse === settings.collapse && trace.nominal === settings.nominal) {
		try {
			const prng = new ScriptedPRNG(trace.prefix, trace.mode);
			const battle = attempt(prng);
			if (stableKey(canonical(battle)) === target) return found(battle, prng);
		} catch (e) {
			if (!(e instanceof ExhaustedTrace)) throw e;
		}
	}
	const rng = mulberry32(0x5eed);
	const walks = opts.setupWalks ?? 2000;
	for (let i = 0; i < walks; i++) {
		const prng = new WalkPRNG(rng, 'extremes');
		const battle = attempt(prng);
		if (stableKey(canonical(battle)) === target) return found(battle, prng);
	}
	const cap = opts.setupMaxBranches ?? 200000;
	let prefix = [];
	let branches = 0;
	while (prefix) {
		if (++branches > cap) {
			throw new Error(`${opts.what}: pinned state not found (${walks} walks, more than ${cap} branches)`);
		}
		const prng = new ScriptedPRNG(prefix, 'extremes');
		const battle = attempt(prng);
		if (stableKey(canonical(battle)) === target) return found(battle, prng);
		prefix = nextPrefix(prng.trace);
	}
	throw new Error(`${opts.what}: no branch reaches the pinned state (${walks} walks, ${branches} branches)`);
}

// ---- exact collapse of secondary-effect rolls ----------------------------------------------

// Showdown draws `random(100)` for every secondary effect and every self-drop and only compares it
// with the effect's chance, which makes one roll 100 branches for the oracle. With
// --collapse-secondaries, `secondaries`/`selfDrops` are recompiled from their own source with that
// draw replaced by `prng.thresholdRoll(chance)`: one two-way decision weighted ceil(chance)/100
// (no decision when the chance is undefined, since the roll is then unused). The distribution is
// unchanged, only the branch count drops. If the vendor source no longer contains the draw exactly
// once, the oracle refuses instead of guessing.
let collapsed = null;

function collapsedActions(actions) {
	if (collapsed) return collapsed;
	const recompile = (fn, from, to) => {
		const src = fn.toString();
		const n = src.split(from).length - 1;
		if (n !== 1) throw new Error(`--collapse-secondaries: ${fn.name} contains "${from}" ${n} times`);
		// eslint-disable-next-line no-new-func
		return new Function(`return function ${src.replace(from, to)}`)();
	};
	const draw = 'const secondaryRoll = this.battle.random(100);';
	collapsed = {
		secondaries: recompile(actions.secondaries, draw,
			'const secondaryRoll = this.battle.prng.thresholdRoll(secondary.chance);'),
		selfDrops: recompile(actions.selfDrops, draw,
			'const secondaryRoll = this.battle.prng.thresholdRoll(moveData.self.chance);'),
	};
	return collapsed;
}

// Puts `prng` in charge of every random call of `battle`, with the oracle's reductions.
function instrument(battle, prng, opts) {
	battle.prng = prng;
	tagDamageRolls(battle, prng);
	if (!opts.keepNominalDraws) collapseNominalTargets(battle);
	if (opts.collapse) Object.assign(battle.actions, collapsedActions(battle.actions));
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
		this.rolls = 0; // damage rolls collapsed to min/max (extremes mode)
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
				this.rolls++;
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
	// A `random(100)` whose only use is `roll < chance` (--collapse-secondaries): 0 stands for
	// every roll below the chance, 99 for every other.
	thresholdRoll(chance) {
		if (chance === undefined) return 0;
		const hits = Math.ceil(chance);
		if (hits >= 100) return 0;
		if (hits <= 0) return 99;
		return this.decide([0, 99], [hits / 100, 1 - hits / 100]);
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

// A random walk through the same decision points: each is drawn by its weights from `rng`.
class WalkPRNG extends ScriptedPRNG {
	constructor(rng, mode) {
		super([], mode);
		this.rng = rng;
	}
	decide(values, weights) {
		let u = this.rng();
		let choice = values.length - 1;
		for (let k = 0; k < weights.length; k++) {
			if (u < weights[k]) {
				choice = k;
				break;
			}
			u -= weights[k];
		}
		this.trace.push({weights, choice});
		return values[choice];
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
// possibly after the residual phase, or for a fainted one flagged while actions are still
// queued (Emergency Exit after its user's own recoil knocked it out: `checkFainted` only flags
// fainted Pokémon once the queue is empty). Every requesting side gets its next `midTurn`
// choice (`{p1: ["switch 3"], p2: []}`); if one has none left the battle stays paused there and
// the paused state (its `request` is `switch`) is the outcome. An end-of-turn replacement
// request (for fainted Pokémon only, the queue empty) is the next decision, not handled here.
function midTurnRequest(side) {
	if (side.requestState !== 'switch') return false;
	const queued = side.battle.queue.list.length > 0;
	return side.active.some((pokemon, i) => pokemon && (pokemon.hp > 0 || queued) && side.activeRequest.forceSwitch[i]);
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

// `opts`: {mode, maxBranches, keepNominalDraws, collapse, traces} (the legacy positional form
// `enumerate(scenario, snapshot, mode, maxBranches, keepNominalDraws)` still works).
function enumerate(scenario, snapshot, opts, maxBranchesArg, keepNominalDrawsArg = false) {
	if (typeof opts === 'string') {
		opts = {mode: opts, maxBranches: maxBranchesArg, keepNominalDraws: keepNominalDrawsArg};
	}
	const {mode, maxBranches} = opts;
	const outcomes = new Map();
	let prefix = [];
	let branches = 0;
	let approximate = false;
	let maxDepth = 0;
	// Extremes mode: each branch with r collapsed rolls stands for about 8^r full-mode branches.
	let fullEstimate = 0;
	while (prefix) {
		if (++branches > maxBranches) throw new Error(`more than ${maxBranches} branches; use --mode extremes or mc`);
		const battle = restore(snapshot);
		const prng = new ScriptedPRNG(prefix, mode);
		instrument(battle, prng, opts);
		const {state, log} = runTurn(battle, scenario);
		const p = prng.trace.reduce((acc, t) => acc * t.weights[t.choice], 1);
		approximate ||= prng.approximate;
		fullEstimate += 8 ** prng.rolls;
		maxDepth = Math.max(maxDepth, prng.trace.length);
		const key = canonicalKey(state);
		const entry = outcomes.get(key);
		if (entry) {
			entry.p += p;
			entry.branches++;
		} else {
			const outcome = {p, branches: 1, state, log};
			if (opts.traces) {
				outcome.trace = {
					mode, collapse: !!opts.collapse, nominal: !opts.keepNominalDraws,
					prefix: prng.trace.map(t => t.choice),
				};
			}
			outcomes.set(key, outcome);
		}
		prefix = nextPrefix(prng.trace);
	}
	return {branches, maxDepth, approximate, outcomes, fullEstimate};
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
	const {snapshot, before, traces} = buildSnapshot(scenario, baseDir, args);
	const setupMs = Number(process.hrtime.bigint() - started) / 1e6;
	const result = args.mode === 'mc' ?
		monteCarlo(scenario, snapshot, args.samples) :
		enumerate(scenario, snapshot, args);
	const elapsedMs = Number(process.hrtime.bigint() - started) / 1e6;
	const outcomes = [...result.outcomes.values()]
		.sort((a, b) => b.p - a.p)
		.map(o => (o.trace ?
			{p: o.p, branches: o.branches, state: o.state, log: o.log, trace: o.trace} :
			{p: o.p, branches: o.branches, state: o.state, log: o.log}));
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
	if (args.collapse) report.collapseSecondaries = true;
	if (args.mode === 'extremes') report.fullBranchEstimate = result.fullEstimate;
	if (scenario.startState || (scenario.setupStates || []).some(Boolean)) {
		report.setupMs = Math.round(setupMs);
		report.startTrace = traces.start;
		report.setupTraces = traces.setup;
	}
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
