'use strict';
// Showdown oracle: the exact outcome distribution of one turn.
//
// Every random call in Showdown goes through battle.prng (random / randomChance / sample /
// shuffle). This script swaps the PRNG for a scripted one and walks every branch depth-first,
// re-running the turn from a serialized snapshot for each branch. Identical end states are
// merged by their canonical form (canonical.cjs), which lab-engine must reproduce exactly.
//
// Usage:
//   node engine/oracle/enumerate.cjs <scenario.json> [--mode full|extremes|fixed|mc] [--roll K]
//                                    [--samples N] [--max-branches N] [--keep-nominal-draws] [--out file]
//                                    [--collapse-secondaries] [--traces]
//                                    [--setup-walks N] [--setup-max-branches N] [--estimate N]
//                                    [--staged] [--setup-staged-max-branches N]
//
// Modes:
//   full      every branch with its exact probability (the parity reference)
//   extremes  damage rolls only take the min and max roll; probabilities are NOT exact
//   fixed     every damage roll takes roll K (--roll, 0..15, lab-engine's ascending index:
//             0 = 85% (the minimum), 7 = 92% (the engine's `Median`), 15 = 100% (the maximum);
//             Showdown's `random(16)` then returns 15 - K). No roll branching at all, everything
//             else exact: lab-engine's `RollMode::Fixed(K)` reproduces it exactly
//             (JJ-heavy-turn-parity: turns too heavy even for `extremes`). The report has `roll`.
//   mc        N natural PRNG runs, empirical frequencies (cross-checks `full`)
//
// --collapse-secondaries: a secondary effect's or self-drop's `random(100) < chance` draw becomes
// one two-way decision (exact; see collapsedActions). --traces: every outcome carries the
// scripted-PRNG trace of its first branch, which replays it (`setupTraces` below).
//
// --staged: the turn is enumerated one action at a time, identical states merging between actions
// (same distribution, far fewer runs on heavy turns; see enumerateStaged). The report has
// `staged: {stages, maxFrontier, merged}`.
//
// --estimate N: instead of enumerating, N uniform random walks through the decision tree of the
// given mode estimate how many branches the enumeration would run (Knuth's estimator: the mean of
// the product of the option counts along a walk; unbiased, but noisy on lopsided trees). The
// report is {mode, roll, estimate: {walks, branches, stderr, maxDepth, msPerBranch}, ...}: how heavy
// a turn is for the plain enumeration of each mode (a diagnostic; the staged runs are far smaller).
//
// Pinned positions (FF-parity-harness): `startState` (canonical state after the leads' switch-ins)
// and `setupStates[k]` (canonical state after setup turn k) keep only the branch of the start or of
// that setup turn that ends in the pinned state, instead of the scenario seed's natural path. The
// branch is found by replaying `startTrace`/`setupTraces[k]` (a trace from an earlier report) if
// given and still valid, else by up to --setup-walks random walks, else (a setup turn) by a staged
// search of that turn, in extremes mode and then with every roll, each up to
// --setup-staged-max-branches runs, dropping stage states that can no longer end in the pin (V3: a
// heavy lead turn, which no report ever supplies a trace for, stays reachable; `findPinnedStaged`),
// else by a depth-first enumeration of up to
// --setup-max-branches branches (extremes mode). The report's `startTrace` and `setupTraces` are the
// branches used, `setupSearch` how each pinned setup turn was found.

const fs = require('node:fs');
const path = require('node:path');
// LAB_ROOT: git worktree 등 vendor/가 없는 체크아웃에서 본 저장소(vendor/pokemon-showdown 포함)를 가리킨다.
const root = process.env.LAB_ROOT ? path.resolve(process.env.LAB_ROOT) : path.resolve(__dirname, '../..');
const {Battle, PRNG, Teams} = require(path.join(root, 'vendor/pokemon-showdown/dist/sim'));
const {canonical, canonicalKey} = require('./canonical.cjs');

function parseArgs(argv) {
	const args = {
		mode: 'full', samples: 20000, maxBranches: 500000, out: null, file: null, keepNominalDraws: false,
		collapse: false, traces: false, setupWalks: 2000, setupMaxBranches: 200000, setupStagedMaxBranches: 200000,
		roll: null,
	};
	for (let i = 0; i < argv.length; i++) {
		const a = argv[i];
		if (a === '--mode') args.mode = argv[++i];
		else if (a === '--samples') args.samples = Number(argv[++i]);
		else if (a === '--roll') args.roll = Number(argv[++i]);
		else if (a === '--max-branches') args.maxBranches = Number(argv[++i]);
		else if (a === '--out') args.out = argv[++i];
		else if (a === '--keep-nominal-draws') args.keepNominalDraws = true;
		else if (a === '--collapse-secondaries') args.collapse = true;
		else if (a === '--traces') args.traces = true;
		else if (a === '--setup-walks') args.setupWalks = Number(argv[++i]);
		else if (a === '--setup-max-branches') args.setupMaxBranches = Number(argv[++i]);
		else if (a === '--setup-staged-max-branches') args.setupStagedMaxBranches = Number(argv[++i]);
		else if (a === '--estimate') args.estimate = Number(argv[++i]);
		else if (a === '--staged') args.staged = true;
		else if (!args.file) args.file = a;
		else throw new Error(`unexpected argument ${a}`);
	}
	if (!args.file) throw new Error('usage: enumerate.cjs <scenario.json> [--mode full|extremes|fixed|mc]');
	if (!['full', 'extremes', 'fixed', 'mc'].includes(args.mode)) throw new Error(`unknown mode ${args.mode}`);
	if ((args.mode === 'fixed') !== (args.roll !== null)) throw new Error('--roll K goes with --mode fixed (and only there)');
	if (args.mode === 'fixed' && !(Number.isInteger(args.roll) && args.roll >= 0 && args.roll <= 15)) {
		throw new Error(`--roll needs an index 0..15, got ${args.roll}`);
	}
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
	const traces = {start: null, setup: [], search: []};
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
			}, pins[k], known[k], {...opts, what: `setup turn ${k + 1}`},
			{snapshot, turn: {p1: c1, p2: c2}, midTurn});
			battle = found.battle;
			traces.setup.push(found.trace);
			traces.search.push(found.how);
		} else {
			battle.makeChoices(c1, c2);
			applyMidTurn(battle, midTurn);
			traces.setup.push(null);
			traces.search.push(null);
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
// has), else, for a setup turn (`staged`: {snapshot, turn, midTurn}), the staged enumeration of
// that turn (`findPinnedStaged`), else a depth-first enumeration in extremes mode (pins recorded
// under min/max rolls are reachable there). Returns that branch's battle, with a real PRNG again,
// its trace and how it was found (`how`).
function findPinned(make, run, pin, trace, opts, staged = null) {
	const target = stableKey(pin);
	const settings = {collapse: !!opts.collapse, nominal: !opts.keepNominalDraws};
	const attempt = prng => {
		const battle = make();
		instrument(battle, prng, opts);
		run(battle);
		return battle;
	};
	const found = (battle, prng, how) => {
		battle.prng = new PRNG(DEFAULT_SEED);
		return {battle, how, trace: {...traceMode(prng), ...settings, prefix: prng.trace.map(t => t.choice)}};
	};
	if (trace && trace.collapse === settings.collapse && trace.nominal === settings.nominal) {
		try {
			const prng = new ScriptedPRNG(trace.prefix, trace.mode, trace.roll ?? null);
			const battle = attempt(prng);
			if (stableKey(canonical(battle)) === target) return found(battle, prng, 'trace');
		} catch (e) {
			if (!(e instanceof ExhaustedTrace)) throw e;
		}
	}
	const rng = mulberry32(0x5eed);
	const walks = opts.setupWalks ?? 2000;
	const walkStart = Date.now();
	for (let i = 0; i < walks; i++) {
		const prng = new WalkPRNG(rng, 'extremes');
		const battle = attempt(prng);
		if (stableKey(canonical(battle)) === target) return found(battle, prng, `walk ${i + 1}`);
	}
	const stagedCap = opts.setupStagedMaxBranches ?? 200000;
	let tried = `${walks} walks`;
	if (process.env.LAB_ORACLE_PROGRESS) {
		console.error(`${opts.what}: ${walks} walks missed the pin (${Math.round((Date.now() - walkStart) / 1000)} s)`);
	}
	// Extremes first (lab-parity pins its outcomes from the engine's min/max-roll distribution); when
	// no extremes branch ends in the pin, every roll (a pin with a middle roll: one recorded under
	// another roll mode, or by an engine that drew a roll the oracle's reduced modes do not).
	for (const mode of staged && stagedCap > 0 ? ['extremes', 'full'] : []) {
		const search = findPinnedStaged(staged, pin, target, {...opts, searchMode: mode}, stagedCap);
		tried += `, staged ${mode} search ${search.note}`;
		if (search.prefix) {
			const prng = new ScriptedPRNG(search.prefix, mode);
			const battle = attempt(prng);
			if (stableKey(canonical(battle)) === target) return found(battle, prng, `staged ${mode} search (${search.note})`);
			throw new Error(`${opts.what}: the staged search's branch does not replay the pinned state`);
		}
		if (!search.complete) break; // over the cap: the depth-first enumeration below is the last resort
		if (mode === 'full') throw new Error(`${opts.what}: no branch reaches the pinned state (${tried})`);
	}
	const cap = opts.setupMaxBranches ?? 200000;
	let prefix = [];
	let branches = 0;
	while (prefix) {
		if (++branches > cap) {
			throw new Error(`${opts.what}: pinned state not found (${tried}, more than ${cap} branches)`);
		}
		const prng = new ScriptedPRNG(prefix, 'extremes');
		const battle = attempt(prng);
		if (stableKey(canonical(battle)) === target) return found(battle, prng, `depth-first branch ${branches}`);
		prefix = nextPrefix(prng.trace);
	}
	throw new Error(`${opts.what}: no branch reaches the pinned state (${tried}, ${branches} branches)`);
}

// The staged search of one pinned setup turn (roll mode `opts.searchMode`, default extremes): the
// turn run one action at a time as in enumerateStaged, depth first, the stage states closest to the pin first (`pinDistance`), each
// distinct stage state expanded once (its future does not depend on how it was reached: the staged
// enumeration's merge), stage states that `pinPruner` shows cannot end in the pin dropped, and
// stopped at the first outcome equal to the pin. Complete (every distinct stage state is expanded
// unless the pin is found first), and it keeps only the visited keys and the pending siblings along
// one path, where the breadth-first enumeration keeps whole stage frontiers. Returns {prefix, note}
// (the branch, which replays the turn unstaged), {complete: true, note} (no branch ends in the pin)
// or {note} (over `cap` runs).
function findPinnedStaged(staged, pin, target, opts, cap) {
	const crypto = require('node:crypto');
	const scenario = {turn: staged.turn, midTurn: staged.midTurn};
	const o = {mode: opts.searchMode || 'extremes', collapse: opts.collapse, keepNominalDraws: opts.keepNominalDraws};
	const prune = pinPruner(pin);
	const distance = pinDistance(pin);
	const visited = new Set();
	let runs = 0;
	let expanded = 0;
	let merged = 0;
	let pruned = 0;
	const started = Date.now();
	const note = () => `${runs} runs, ${expanded} states expanded, ${merged} merged, ${pruned} pruned, ` +
		`${Math.round((Date.now() - started) / 1000)} s`;
	const stack = [[{snapshot: staged.snapshot, prefix: [], used: {p1: 0, p2: 0}, start: true}]];
	while (stack.length) {
		const level = stack[stack.length - 1];
		const entry = level.pop();
		if (!level.length) stack.pop();
		expanded++;
		const children = [];
		let stagePrefix = [];
		while (stagePrefix) {
			if (++runs > cap) return {note: `over ${cap} runs (${note()})`};
			const {battle, prng, used, more} = runStage(entry, stagePrefix, scenario, o);
			const prefix = entry.prefix.concat(prng.trace.map(t => t.choice));
			if (more) {
				if (prune(battle)) {
					pruned++;
				} else {
					battle.prng = new PRNG(DEFAULT_SEED);
					const state = battle.toJSON();
					const key = crypto.createHash('sha256').update(stageKey(state, used)).digest('base64');
					if (visited.has(key)) {
						merged++;
					} else {
						visited.add(key);
						children.push({packed: pack(JSON.stringify(state)), prefix, used, start: false, score: distance(battle)});
					}
				}
			} else if (stableKey(canonical(battle)) === target) {
				return {prefix, note: note()};
			}
			stagePrefix = nextPrefix(prng.trace);
		}
		if (children.length) {
			children.sort((a, b) => b.score - a.score); // pop() takes the closest first
			stack.push(children);
		}
		if (process.env.LAB_ORACLE_PROGRESS && expanded % 500 === 0) console.error(`${opts.what}: staged search ${note()}`);
	}
	return {complete: true, note: note()};
}

// How far a stage state looks from the pinned outcome (a search order only, not a bound): HP below
// the pin (only healing brings it back) weighs more than HP above it (damage still to come), then
// status, item, boosts and the field.
function pinDistance(pin) {
	const want = new Map();
	pin.sides.forEach((side, i) => {
		for (const mon of side.pokemon) want.set(`p${i + 1}|${mon.name}`, mon);
	});
	return battle => {
		const f = battle.field;
		let d = ((f.weather || '') !== pin.field.weather ? 1 : 0) + ((f.terrain || '') !== pin.field.terrain ? 1 : 0);
		for (const side of battle.sides) {
			for (const mon of side.pokemon) {
				const pinned = want.get(`${side.id}|${mon.name}`);
				if (!pinned) continue;
				const hp = (mon.hp - pinned.hp) / Math.max(1, mon.maxhp);
				d += hp < 0 ? -8 * hp : hp;
				if ((mon.status || '') !== pinned.status) d += 1;
				if ((mon.item || '') !== pinned.item) d += 0.5;
				if (mon.isActive && pinned.boosts) {
					for (const k of ['atk', 'def', 'spa', 'spd', 'spe', 'accuracy', 'evasion']) {
						d += 0.25 * Math.abs((mon.boosts[k] || 0) - (pinned.boosts[k] || 0));
					}
				}
			}
		}
		return d;
	};
}

// Sound pruning for a pinned turn: a stage state that already contradicts the pin in something that
// cannot change back before the turn ends. Within one turn (1) a fainted Pokémon stays fainted,
// unless its side knows Revival Blessing; (2) move PP only goes down, unless PP can come back or the
// move slots can change: a Leppa Berry anywhere (held or last held: Harvest, Recycle, Pickup, Fling,
// Bug Bite), or Transform / Imposter (a transformed Pokémon's pinned PP are the copied moves'). The
// pin is a canonical state (Pokémon keyed by side and name).
function pinPruner(pin) {
	const want = new Map();
	pin.sides.forEach((side, i) => {
		for (const mon of side.pokemon) want.set(`p${i + 1}|${mon.name}`, mon);
	});
	return battle => {
		const all = battle.sides.flatMap(side => side.pokemon);
		const ppMonotone = !all.some(mon => mon.item === 'leppaberry' || mon.lastItem === 'leppaberry' ||
			mon.baseAbility === 'imposter' || mon.ability === 'imposter' || mon.transformed ||
			mon.baseMoveSlots.some(m => m.id === 'transform'));
		for (const side of battle.sides) {
			const revival = side.pokemon.some(mon => mon.baseMoveSlots.some(m => m.id === 'revivalblessing'));
			for (const mon of side.pokemon) {
				const pinned = want.get(`${side.id}|${mon.name}`);
				if (!pinned) continue;
				if (!mon.hp && pinned.hp > 0 && !revival) return true;
				if (!ppMonotone) continue;
				for (const slot of mon.moveSlots) {
					const pp = pinned.pp?.[slot.id];
					if (pp !== undefined && slot.pp < pp) return true;
				}
			}
		}
		return false;
	};
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
// `roll`: the fixed roll index of `fixed` mode (lab-engine's ascending index, 0 = 85%).
class ScriptedPRNG {
	constructor(prefix, mode, roll = null) {
		this.prefix = prefix;
		this.mode = mode;
		this.roll = roll;
		if (mode === 'fixed' && !(Number.isInteger(roll) && roll >= 0 && roll <= 15)) {
			throw new Error(`fixed mode needs a roll index 0..15, got ${roll}`);
		}
		this.trace = []; // {weights, choice}
		this.approximate = false;
		this.nextIsRoll = false;
		this.rolls = 0; // damage rolls collapsed to min/max (extremes mode) or fixed (fixed mode)
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
			// Showdown's roll r is the multiplier 100 - r: the engine's ascending index K is r = 15 - K.
			if (this.mode === 'fixed' && n === 16) {
				this.approximate = true;
				this.rolls++;
				return lo + 15 - this.roll;
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

// A uniform random walk (every option equally likely, whatever its weight): Knuth's estimator.
class UniformWalkPRNG extends ScriptedPRNG {
	constructor(rng, mode, roll) {
		super([], mode, roll);
		this.rng = rng;
	}
	decide(values, weights) {
		const choice = Math.min(values.length - 1, Math.floor(this.rng() * values.length));
		this.trace.push({weights, choice});
		return values[choice];
	}
}

// Estimates the number of branches `enumerate(scenario, snapshot, opts)` would run from `walks`
// uniform random walks (see --estimate).
function estimateBranches(scenario, snapshot, opts, walks) {
	const rng = mulberry32(0xe57);
	let sum = 0;
	let sumSq = 0;
	let maxDepth = 0;
	const started = process.hrtime.bigint();
	for (let i = 0; i < walks; i++) {
		const battle = restore(snapshot);
		const prng = new UniformWalkPRNG(rng, opts.mode, opts.roll ?? null);
		instrument(battle, prng, opts);
		runTurn(battle, scenario);
		const product = prng.trace.reduce((acc, t) => acc * t.weights.length, 1);
		sum += product;
		sumSq += product * product;
		maxDepth = Math.max(maxDepth, prng.trace.length);
	}
	const ms = Number(process.hrtime.bigint() - started) / 1e6;
	const mean = sum / walks;
	const sd = Math.sqrt(Math.max(0, sumSq / walks - mean * mean));
	return {walks, branches: Math.round(mean), stderr: Math.round(sd / Math.sqrt(walks)), maxDepth,
		msPerBranch: +(ms / walks).toFixed(3)};
}

// Showdown's serializer (sim/state.ts) writes a reference to a dex Move as `[DataMove:<id>]` (the
// class's name, `Dex.Move = DataMove`) but `fromRef` only reads `[Move:<id>]`, so a plain Move object
// in the battle state comes back as that literal string. The only one there is between two actions
// is a queued switch's `sourceEffect` (the self-switch move, from `switchFlag`), which a two-switch
// mid-turn commit carries across a stage of the staged enumeration: its `|switch|` log line then
// read `[from] [DataMove:uturn]`, and a Baton Pass / Shed Tail switch would lose its `selfSwitch`
// copy flag. Between turns (the plain enumeration's snapshots) no such object exists. (V10)
function restore(snapshot) {
	return Battle.fromJSON(snapshot.replaceAll('"[DataMove:', '"[Move:'));
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

// A trace's mode fields: `{mode}`, plus `roll` in fixed mode (a replay needs it).
function traceMode(prng) {
	return prng.mode === 'fixed' ? {mode: prng.mode, roll: prng.roll} : {mode: prng.mode};
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
	// Extremes mode: each branch with r collapsed rolls stands for about 8^r full-mode branches;
	// fixed mode: 16^r.
	const perRoll = mode === 'fixed' ? 16 : 8;
	let fullEstimate = 0;
	while (prefix) {
		if (++branches > maxBranches) throw new Error(`more than ${maxBranches} branches; use --mode extremes or mc`);
		const battle = restore(snapshot);
		const prng = new ScriptedPRNG(prefix, mode, opts.roll ?? null);
		instrument(battle, prng, opts);
		const {state, log} = runTurn(battle, scenario);
		const p = prng.trace.reduce((acc, t) => acc * t.weights[t.choice], 1);
		approximate ||= prng.approximate;
		fullEstimate += perRoll ** prng.rolls;
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
					...traceMode(prng), collapse: !!opts.collapse, nominal: !opts.keepNominalDraws,
					prefix: prng.trace.map(t => t.choice),
				};
			}
			outcomes.set(key, outcome);
		}
		prefix = nextPrefix(prng.trace);
	}
	return {branches, maxDepth, approximate, outcomes, fullEstimate};
}

// ---- staged enumeration (--staged) ---------------------------------------------------------
//
// A doubles turn with a Speed tie between active Pokémon (a mirror match) or several spread moves
// makes the plain enumeration above run the product of every action's branch counts: every
// `speedSort` tie in an `eachEvent('Update')` after every hit is a 50/50 decision that mostly
// changes nothing, and each is re-enumerated under every combination of the other actions'
// decisions. With --staged the turn runs one queued action at a time (`steppedTurnLoop`, Showdown's
// `turnLoop` stopping after each action); between actions the battle is serialized, branches that
// reach the same serialized state (the log and the PRNG aside) merge with their probabilities
// added, and the next action is enumerated once from each distinct state: the engine's stage
// frontier. Every branch still runs Showdown's own code from a Showdown snapshot, and a merged
// state's future does not depend on which branch reached it, so the distribution is the same as
// the plain enumeration's (checked against the oracle fixtures); only the number of runs drops, to
// about the sum over actions of (distinct states before it) x (its own branches). What the plain
// enumeration trusts in Showdown (`Battle.fromJSON` of a snapshot between turns) this also trusts
// between actions. An outcome's trace is the concatenated decisions of one path through the
// stages, which replays the turn unstaged.

// Showdown's `Battle.turnLoop` (sim/battle.ts) running one queued action per call: the loop's
// `while` becomes the caller's, which serializes the battle between the calls. The two log lines
// the real loop adds on entry (`|` and `|t:|`) are left out (log only; `turnLog` drops them).
function steppedTurnLoop() {
	if (this.requestState) this.requestState = '';
	if (!this.midTurn) {
		this.queue.insertChoice({choice: 'beforeTurn'});
		this.queue.addChoice({choice: 'residual'});
		this.midTurn = true;
	}
	const action = this.queue.shift();
	if (action) {
		this.runAction(action);
		if (this.requestState || this.ended) return;
		if (this.queue.list.length) return; // between two actions: the next stage resumes here
	}
	this.endTurn();
	this.midTurn = false;
	this.queue.clear();
}

// Serialized-battle fields that only concern the log, the PRNG or the client, left out of the
// merge key (a merged state keeps the first branch's values, consistent with its own log).
const STAGE_KEY_SKIP = new Set([
	'log', 'inputLog', 'messageLog', 'sentLogPos', 'sentEnd', 'sentRequests', 'lastMoveLine', 'prng',
	'prngSeed', 'hints',
]);

// What the turn does after a stage: 'continue' (between two actions, or a mid-turn switch request
// the scenario's `midTurn` answers next) or 'done' (the turn is over, or waits for a choice the
// scenario does not make: that state is the outcome, as in `runTurn`).
function stageState(battle, midTurn, used) {
	if (battle.ended) return 'done';
	const requesting = battle.sides.filter(midTurnRequest);
	if (requesting.length) {
		const answered = requesting.every(side => ((midTurn && midTurn[side.id]) || [])[used[side.id]] !== undefined);
		return answered ? 'continue' : 'done';
	}
	if (battle.midTurn && !battle.requestState && battle.queue.list.length) return 'continue';
	return 'done';
}

// One round of `applyMidTurn`: every requesting side makes its next `midTurn` choice; the last one
// commits them and runs the next action (through `steppedTurnLoop`).
function midTurnRound(battle, midTurn, used) {
	const requesting = battle.sides.filter(midTurnRequest);
	const choices = requesting.map(side => ((midTurn && midTurn[side.id]) || [])[used[side.id]]);
	requesting.forEach((side, i) => {
		used[side.id]++;
		if (!battle.choose(side.id, choices[i])) {
			throw new Error(`${side.id}: mid-turn choice "${choices[i]}" rejected: ${battle.log.slice(-1)}`);
		}
	});
}

// One branch of one stage of a staged turn: the entry's state restored and its next step run with
// `stagePrefix` as the scripted decisions (the turn's choices at the start, a round of mid-turn
// switch choices, or the next queued action). `more`: the turn goes on from this state.
function runStage(entry, stagePrefix, scenario, opts) {
	const battle = restore(entry.packed ? unpack(entry.packed) : entry.snapshot);
	const prng = new ScriptedPRNG(stagePrefix, opts.mode, opts.roll ?? null);
	instrument(battle, prng, opts);
	battle.turnLoop = steppedTurnLoop;
	const used = {...entry.used};
	if (entry.start) battle.makeChoices(scenario.turn.p1, scenario.turn.p2);
	else if (battle.sides.some(midTurnRequest)) midTurnRound(battle, scenario.midTurn, used);
	else battle.turnLoop();
	return {battle, prng, used, more: stageState(battle, scenario.midTurn, used) === 'continue'};
}

// What identifies a stage state for merging: its serialization, log and PRNG aside, and the mid-turn
// choices used so far.
function stageKey(state, used) {
	const keyed = {used};
	for (const [k, v] of Object.entries(state)) if (!STAGE_KEY_SKIP.has(k)) keyed[k] = v;
	return JSON.stringify(keyed);
}

// A stage state waits in the frontier deflated (a snapshot is tens of kilobytes of JSON, most of it
// the log; heavy turns keep tens of thousands of them).
const zlib = require('node:zlib');
const pack = text => zlib.deflateRawSync(text, {level: 1});
const unpack = packed => zlib.inflateRawSync(packed).toString();

function enumerateStaged(scenario, snapshot, opts) {
	const {mode, maxBranches} = opts;
	const crypto = require('node:crypto');
	const started = Date.now();
	const outcomes = new Map();
	const perRoll = mode === 'fixed' ? 16 : 8;
	let branches = 0;
	let approximate = false;
	let maxDepth = 0;
	let fullEstimate = 0;
	let stages = 0;
	let maxFrontier = 1;
	let merged = 0;
	const logStart = JSON.parse(snapshot).log.length;
	let frontier = [{snapshot, p: 1, prefix: [], used: {p1: 0, p2: 0}, start: true}];
	while (frontier.length) {
		stages++;
		const next = new Map();
		for (const entry of frontier) {
			let stagePrefix = [];
			while (stagePrefix) {
				if (++branches > maxBranches) throw new Error(`more than ${maxBranches} branches; use --mode extremes or mc`);
				const {battle, prng, used, more} = runStage(entry, stagePrefix, scenario, opts);
				const p = prng.trace.reduce((acc, t) => acc * t.weights[t.choice], entry.p);
				approximate ||= prng.approximate;
				fullEstimate += perRoll ** prng.rolls;
				const prefix = entry.prefix.concat(prng.trace.map(t => t.choice));
				maxDepth = Math.max(maxDepth, prefix.length);
				if (more) {
					battle.prng = new PRNG(DEFAULT_SEED);
					const state = battle.toJSON();
					const key = crypto.createHash('sha256').update(stageKey(state, used)).digest('base64');
					const found = next.get(key);
					if (found) {
						found.p += p;
						merged++;
					} else {
						next.set(key, {packed: pack(JSON.stringify(state)), p, prefix, used, start: false});
					}
				} else {
					const state = canonical(battle);
					const key = canonicalKey(state);
					const found = outcomes.get(key);
					if (found) {
						found.p += p;
						found.branches++;
					} else {
						const outcome = {p, branches: 1, state, log: turnLog(battle.log.slice(logStart))};
						if (opts.traces) {
							outcome.trace = {
								...traceMode(prng), collapse: !!opts.collapse, nominal: !opts.keepNominalDraws, prefix,
							};
						}
						outcomes.set(key, outcome);
					}
				}
				stagePrefix = nextPrefix(prng.trace);
			}
		}
		frontier = [...next.values()];
		maxFrontier = Math.max(maxFrontier, frontier.length);
		if (process.env.LAB_ORACLE_PROGRESS) {
			console.error(`staged: stage ${stages}, frontier ${frontier.length}, ${branches} runs, ${merged} merged, ` +
				`${outcomes.size} outcomes, ${Math.round((Date.now() - started) / 1000)} s`);
		}
	}
	return {branches, maxDepth, approximate, outcomes, fullEstimate, staged: {stages, maxFrontier, merged}};
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
	if (args.estimate) {
		if (args.mode === 'mc') throw new Error('--estimate goes with an enumerating mode');
		const estimate = estimateBranches(scenario, snapshot, args, args.estimate);
		const report = {
			scenario: path.relative(root, scenarioPath).replaceAll('\\', '/'),
			mode: args.mode,
			...(args.mode === 'fixed' ? {roll: args.roll} : {}),
			collapseSecondaries: !!args.collapse,
			estimate,
			setupMs: Math.round(setupMs),
			startTrace: traces.start,
			setupTraces: traces.setup,
		};
		const text = JSON.stringify(report, null, 1);
		if (args.out) fs.writeFileSync(args.out, text);
		else process.stdout.write(text + '\n');
		console.error(`${args.mode}${args.mode === 'fixed' ? ` ${args.roll}` : ''}: about ${estimate.branches} ` +
			`branches (± ${estimate.stderr}, ${estimate.walks} walks, ${estimate.msPerBranch} ms each)`);
		return;
	}
	if (args.staged && args.mode === 'mc') throw new Error('--staged goes with an enumerating mode');
	const result = args.mode === 'mc' ?
		monteCarlo(scenario, snapshot, args.samples) :
		args.staged ? enumerateStaged(scenario, snapshot, args) : enumerate(scenario, snapshot, args);
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
		...(args.mode === 'fixed' ? {roll: args.roll} : {}),
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
	if (result.staged) report.staged = result.staged;
	if (args.mode === 'extremes' || args.mode === 'fixed') report.fullBranchEstimate = result.fullEstimate;
	if (scenario.startState || (scenario.setupStates || []).some(Boolean)) {
		report.setupMs = Math.round(setupMs);
		report.startTrace = traces.start;
		report.setupTraces = traces.setup;
		report.setupSearch = traces.search;
	}
	const text = JSON.stringify(report, null, 1);
	if (args.out) fs.writeFileSync(args.out, text);
	else process.stdout.write(text + '\n');
	console.error(`${args.mode}${args.mode === 'fixed' ? ` ${args.roll}` : ''}: ${result.branches} branches -> ${outcomes.length} outcomes, ` +
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

module.exports = {
	buildSnapshot, enumerate, enumerateStaged, monteCarlo, ScriptedPRNG, nextPrefix, loadTeam, readJSON, sourceCommit,
	// the staged enumeration's pieces, for check-roundtrip.cjs (V10)
	DEFAULT_SEED, instrument, restore, steppedTurnLoop, stageState, midTurnRequest, midTurnRound, mulberry32,
	ExhaustedTrace,
};
if (require.main === module) main();
