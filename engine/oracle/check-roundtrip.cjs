'use strict';
// V10 (staged serializer faithful): a per-branch check of the one premise of `enumerate.cjs --staged`,
// that Showdown's `Battle.toJSON`/`Battle.fromJSON` between two actions of a turn keeps everything
// the rest of the turn depends on.
//
// For random branches of each scenario's turn, the turn is played twice with the same decisions:
//   - live: one battle, run one queued action at a time (`steppedTurnLoop`) but never serialized
//     (the plain enumeration's path; stepping only splits Showdown's `turnLoop` `while`);
//   - round-tripped: after every action the battle goes through `toJSON` -> `fromJSON` and the
//     next action runs on the restored battle with a fresh scripted PRNG over the remaining
//     decisions (exactly what `enumerateStaged` does between two stages).
// The live run draws its decisions from a random walk (half the walks by the decisions'
// probabilities, half uniformly so rare branches such as crits, misses and secondary effects come
// up); the round-tripped run replays that walk's choices. At every action boundary and at the end
// the two runs must have consumed the same decision points (same number, same weights, same
// choices) and serialize to the same state: every field `toJSON` writes, the log included (its
// `|t:|` timestamps aside), compared with the object keys of battle / side / Pokémon / move
// objects sorted (their order carries no meaning) and, separately, the order of volatiles, side
// conditions, slot conditions and pseudo-weathers kept (it is the order of their handlers). A field
// the serializer drops or rebuilds differently shows as a state difference at the next boundary
// (the restored battle serializes its default) or, if it only changes behaviour, as a different
// decision sequence or a later state difference. What the check cannot see: a value the serializer
// drops and that nothing later in the turn reads (e.g. object-valued fields of a finished move's
// ActiveMove kept in `lastMove`, which `toJSON` never writes); see the V10 table in the report.
//
// The report also counts, over every boundary of every live run, which turn-transient features
// were present when the battle was serialized ("boundary features": a queued move with a modified
// priority, a pending switch request, a Pokémon hit this turn, ...): what the corpus actually
// exercises across a stage boundary.
//
// Usage: LAB_ROOT=<checkout with vendor/> node engine/oracle/check-roundtrip.cjs
//            [--dir <scenario dir>]... [--scenario <file>]... [--walks N] [--seed S]
//            [--shard i/n] [--only <substring>] [--json <out.json>] [--mode full|extremes|fixed]
//            [--roll K] [--setup-walks N] [--setup-max-branches N]
// Without --dir/--scenario: the scenarios of every oracle fixture (engine/oracle/expected/*.json,
// the same set check-staged.cjs runs; `--keep-nominal-draws` when the scenario's description says
// so). Pinned positions (lab-parity's) replay their recorded setup traces; --setup-walks /
// --setup-max-branches bound the search when a trace is missing (defaults 200 / 20,000: a position
// the parity sweep could not set up fails fast here). Exit code 1 if any branch differs or a
// scenario errors.

const fs = require('node:fs');
const path = require('node:path');
const root = process.env.LAB_ROOT ? path.resolve(process.env.LAB_ROOT) : path.resolve(__dirname, '../..');
const {PRNG} = require(path.join(root, 'vendor/pokemon-showdown/dist/sim'));
const E = require('./enumerate.cjs');
const {canonical, canonicalKey} = require('./canonical.cjs');

const repo = path.resolve(__dirname, '../..');

function parseArgs(argv) {
	const args = {
		dirs: [], scenarios: [], walks: 8, seed: 0x10a1, shard: [0, 1], only: null, json: null, mode: 'full',
		roll: null, setupWalks: 200, setupMaxBranches: 20000,
	};
	for (let i = 0; i < argv.length; i++) {
		const a = argv[i];
		if (a === '--dir') args.dirs.push(argv[++i]);
		else if (a === '--scenario') args.scenarios.push(argv[++i]);
		else if (a === '--walks') args.walks = Number(argv[++i]);
		else if (a === '--seed') args.seed = Number(argv[++i]);
		else if (a === '--shard') args.shard = argv[++i].split('/').map(Number);
		else if (a === '--only') args.only = argv[++i];
		else if (a === '--json') args.json = argv[++i];
		else if (a === '--mode') args.mode = argv[++i];
		else if (a === '--roll') args.roll = Number(argv[++i]);
		else if (a === '--setup-walks') args.setupWalks = Number(argv[++i]);
		else if (a === '--setup-max-branches') args.setupMaxBranches = Number(argv[++i]);
		else throw new Error(`unexpected argument ${a}`);
	}
	if (!['full', 'extremes', 'fixed'].includes(args.mode)) throw new Error(`unknown mode ${args.mode}`);
	if ((args.mode === 'fixed') !== (args.roll !== null)) throw new Error('--roll K goes with --mode fixed');
	return args;
}

// ---- which scenarios ------------------------------------------------------------------------

function fixtureScenarios() {
	const dir = path.join(__dirname, 'expected');
	const seen = new Set();
	const out = [];
	for (const f of fs.readdirSync(dir).filter(f => f.endsWith('.json')).sort()) {
		const fixture = E.readJSON(path.join(dir, f));
		if (!fixture.scenario) continue;
		// Some older fixtures name their scenario by file name only.
		let file = path.resolve(repo, fixture.scenario);
		if (!fs.existsSync(file)) file = path.join(__dirname, 'scenarios', path.basename(fixture.scenario));
		if (seen.has(file)) continue;
		seen.add(file);
		out.push(file);
	}
	return out;
}

function scenarioFiles(args) {
	let files = [...args.scenarios.map(f => path.resolve(f))];
	for (const d of args.dirs) {
		for (const f of fs.readdirSync(d).filter(f => f.endsWith('.json')).sort()) files.push(path.resolve(d, f));
	}
	if (!args.dirs.length && !args.scenarios.length) files = fixtureScenarios();
	return files
		.filter(f => !args.only || f.includes(args.only))
		.filter((f, i) => i % args.shard[1] === args.shard[0]);
}

// ---- comparable serialized state --------------------------------------------------------------

function sortKeys(v) {
	if (Array.isArray(v)) return v.map(sortKeys);
	if (v && typeof v === 'object') {
		const out = {};
		for (const k of Object.keys(v).sort()) out[k] = sortKeys(v[k]);
		return out;
	}
	return v;
}

// The key order that means something: handler order follows insertion order of these tables.
function orderSignature(s) {
	const sig = {pseudoWeather: Object.keys(s.field.pseudoWeather || {})};
	for (const side of s.sides) {
		sig[side.id] = {
			sideConditions: Object.keys(side.sideConditions || {}),
			slotConditions: (side.slotConditions || []).map(t => Object.keys(t || {})),
			volatiles: side.pokemon.map(p => Object.keys(p.volatiles || {})),
		};
	}
	return sig;
}

function snapshotOf(battle) {
	const s = battle.toJSON();
	delete s.prng; // a scripted PRNG on both runs: 'scripted'
	s.log = s.log.filter(l => !l.startsWith('|t:|'));
	return s;
}

function comparable(s) {
	return JSON.stringify(sortKeys(s)) + '\n' + JSON.stringify(orderSignature(s));
}

// First differing path between two (sorted) values, for the report.
function firstDiff(a, b, at = '') {
	if (a === b) return null;
	if (typeof a !== typeof b || a === null || b === null || typeof a !== 'object' || Array.isArray(a) !== Array.isArray(b)) {
		return {at, live: short(a), roundTripped: short(b)};
	}
	const keys = [...new Set([...Object.keys(a), ...Object.keys(b)])];
	for (const k of keys) {
		const d = firstDiff(a[k], b[k], `${at}.${k}`);
		if (d) return d;
	}
	return null;
}

function short(v) {
	const s = JSON.stringify(v);
	return s === undefined ? 'undefined' : s.length > 160 ? s.slice(0, 160) + '…' : s;
}

// ---- boundary features ----------------------------------------------------------------------

const ACTION_ORDERS = {
	team: 1, start: 2, instaswitch: 3, beforeTurn: 4, beforeTurnMove: 5, revivalblessing: 6, runSwitch: 101, switch: 103,
	megaEvo: 104, megaEvoX: 104, megaEvoY: 104, runDynamax: 105, terastallize: 106, priorityChargeMove: 107, shift: 200,
	residual: 300,
};

// What turn-transient state the serialized battle `s` carries across this boundary.
function features(s) {
	const f = new Set();
	if (s.requestState === 'switch') f.add('battle.requestState=switch');
	if (s.lastSuccessfulMoveThisTurn) f.add('battle.lastSuccessfulMoveThisTurn');
	if (s.faintQueue?.length) f.add('battle.faintQueue(non-empty)');
	if (s.activeMove) f.add('battle.activeMove(non-null)');
	if (s.activePokemon) f.add('battle.activePokemon(non-null)');
	if (s.activeTarget) f.add('battle.activeTarget(non-null)');
	if (s.effect?.id) f.add('battle.effect(non-empty)');
	if (s.event?.id) f.add('battle.event(non-empty)');
	if (s.eventDepth) f.add('battle.eventDepth>0');
	if (s.quickClawRoll) f.add('battle.quickClawRoll');
	if (s.lastMove) {
		f.add('battle.lastMove');
		for (const k of Object.keys(s.lastMove)) if (!['move', 'hit'].includes(k)) f.add(`battle.lastMove.${k}`);
	}
	for (const a of s.queue || []) {
		f.add(`queue.${a.choice}`);
		if (a.order !== undefined && ACTION_ORDERS[a.choice] !== undefined ? a.order !== ACTION_ORDERS[a.choice] :
			a.order !== undefined && a.order !== 200) f.add(`queue.${a.choice}.order=${a.order}`);
		if (a.sourceEffect) f.add(`queue.${a.choice}.sourceEffect`);
		if (a.fractionalPriority) f.add(`queue.${a.choice}.fractionalPriority`);
		if (a.move && typeof a.move === 'object') {
			for (const k of Object.keys(a.move)) if (!['move', 'hit'].includes(k)) f.add(`queue.move.ActiveMove.${k}`);
		}
	}
	for (const side of s.sides) {
		if (side.faintedThisTurn) f.add('side.faintedThisTurn');
		if (side.choice?.actions?.length) f.add('side.choice.actions(non-empty)');
		if (Object.keys(side.sideConditions || {}).length) f.add('side.sideConditions');
		if ((side.slotConditions || []).some(t => Object.keys(t || {}).length)) f.add('side.slotConditions');
		for (const p of side.pokemon) {
			const active = p.isActive;
			if (p.moveThisTurnResult !== undefined && active) f.add(`pokemon.moveThisTurnResult=${p.moveThisTurnResult}`);
			if (p.moveThisTurn) f.add('pokemon.moveThisTurn');
			if (p.hurtThisTurn !== null && p.hurtThisTurn !== undefined) f.add('pokemon.hurtThisTurn');
			if ((p.attackedBy || []).some(x => x.thisTurn)) f.add('pokemon.attackedBy(thisTurn)');
			if (p.timesAttacked) f.add('pokemon.timesAttacked>0');
			if (p.statsRaisedThisTurn) f.add('pokemon.statsRaisedThisTurn');
			if (p.statsLoweredThisTurn) f.add('pokemon.statsLoweredThisTurn');
			if (p.usedItemThisTurn) f.add('pokemon.usedItemThisTurn');
			if (p.newlySwitched && active) f.add('pokemon.newlySwitched(active)');
			if (p.beingCalledBack) f.add('pokemon.beingCalledBack');
			if (p.switchFlag) f.add('pokemon.switchFlag');
			if (p.forceSwitchFlag) f.add('pokemon.forceSwitchFlag');
			if (p.skipBeforeSwitchOutEventFlag) f.add('pokemon.skipBeforeSwitchOutEventFlag');
			if (p.draggedIn !== null && p.draggedIn !== undefined) f.add('pokemon.draggedIn');
			if (p.baseMoveSlots) f.add('pokemon.baseMoveSlots(!=moveSlots)');
			if (p.illusion) f.add('pokemon.illusion');
			if (p.transformed) f.add('pokemon.transformed');
			if (p.lastMoveTargetLoc !== undefined) f.add('pokemon.lastMoveTargetLoc');
			if (p.duringMove) f.add('pokemon.duringMove');
			if (p.canMegaEvo === false) f.add('pokemon.canMegaEvo=false');
			if (p.lastMove) {
				f.add('pokemon.lastMove');
				for (const k of Object.keys(p.lastMove)) if (!['move', 'hit'].includes(k)) f.add(`pokemon.lastMove.${k}`);
			}
			if (p.lastMoveUsed) f.add('pokemon.lastMoveUsed');
			for (const [id, v] of Object.entries(p.volatiles || {})) {
				if (v.source) f.add('pokemon.volatile.source');
				if (v.linkedPokemon) f.add('pokemon.volatile.linkedPokemon');
				if (v.sourceEffect && typeof v.sourceEffect === 'object') f.add('pokemon.volatile.sourceEffect(ActiveMove)');
				if (v.duration === 1) f.add(`pokemon.volatile(duration 1)`);
				if (id === 'stall' || id === 'protect') f.add(`pokemon.volatile.${id}`);
			}
			for (const k of Object.keys(p.itemState || {})) if (!['id', 'target', 'effectOrder'].includes(k)) f.add(`pokemon.itemState.${k}`);
			for (const k of Object.keys(p.abilityState || {})) if (!['id', 'target', 'effectOrder'].includes(k)) f.add(`pokemon.abilityState.${k}`);
		}
	}
	return f;
}

// ---- one branch -----------------------------------------------------------------------------

// A random walk through the turn's decision points: each drawn by its probability, or uniformly.
class Walk extends E.ScriptedPRNG {
	constructor(rng, mode, roll, uniform) {
		super([], mode, roll);
		this.rng = rng;
		this.uniform = uniform;
	}
	decide(values, weights) {
		let choice = values.length - 1;
		if (this.uniform) {
			choice = Math.min(values.length - 1, Math.floor(this.rng() * values.length));
		} else {
			let u = this.rng();
			for (let k = 0; k < weights.length; k++) {
				if (u < weights[k]) {
					choice = k;
					break;
				}
				u -= weights[k];
			}
		}
		this.trace.push({weights, choice});
		return values[choice];
	}
}

function step(battle, scenario, used) {
	if (battle.sides.some(E.midTurnRequest)) E.midTurnRound(battle, scenario.midTurn, used);
	else battle.turnLoop();
}

function prepare(battle, prng, opts) {
	E.instrument(battle, prng, opts);
	battle.turnLoop = E.steppedTurnLoop;
	return battle;
}

function liveRun(scenario, snapshot, opts, prng) {
	const battle = prepare(E.restore(snapshot), prng, opts);
	const used = {p1: 0, p2: 0};
	const boundaries = [];
	battle.makeChoices(scenario.turn.p1, scenario.turn.p2);
	while (E.stageState(battle, scenario.midTurn, used) === 'continue') {
		const s = snapshotOf(battle);
		boundaries.push({at: prng.trace.length, state: comparable(s), sorted: sortKeys(s), features: features(s)});
		step(battle, scenario, used);
	}
	const s = snapshotOf(battle);
	return {trace: prng.trace, boundaries, final: comparable(s), sorted: sortKeys(s), outcome: canonicalKey(canonical(battle))};
}

function sameDecisions(a, b) {
	if (a.length !== b.length) return false;
	return a.every((t, i) => t.choice === b[i].choice && t.weights.length === b[i].weights.length &&
		t.weights.every((w, j) => w === b[i].weights[j]));
}

// Replays the live run's choices with a toJSON/fromJSON round trip at every boundary. Returns null
// when both runs agree, else the first difference.
function roundTripRun(scenario, snapshot, opts, live) {
	const choices = live.trace.map(t => t.choice);
	const mode = opts.mode;
	const roll = opts.roll ?? null;
	let prng = new E.ScriptedPRNG(choices, mode, roll);
	let battle = prepare(E.restore(snapshot), prng, opts);
	const used = {p1: 0, p2: 0};
	const trace = [];
	let k = 0;
	try {
		battle.makeChoices(scenario.turn.p1, scenario.turn.p2);
		while (E.stageState(battle, scenario.midTurn, used) === 'continue') {
			trace.push(...prng.trace);
			const want = live.boundaries[k];
			if (!want) return {boundary: k, what: 'the round-tripped run has more action boundaries'};
			const s = snapshotOf(battle);
			if (!sameDecisions(trace, live.trace.slice(0, want.at))) {
				return {boundary: k, what: 'different decisions before this boundary'};
			}
			if (comparable(s) !== want.state) {
				return {boundary: k, what: 'different state', diff: firstDiff(want.sorted, sortKeys(s)) ||
					{at: '(key order of volatiles / conditions)'}};
			}
			battle.prng = new PRNG(E.DEFAULT_SEED);
			const json = JSON.stringify(battle.toJSON());
			prng = new E.ScriptedPRNG(choices.slice(trace.length), mode, roll);
			battle = prepare(E.restore(json), prng, opts);
			step(battle, scenario, used);
			k++;
		}
	} catch (e) {
		if (e instanceof E.ExhaustedTrace) return {boundary: k, what: `decisions diverged: ${e.message}`};
		throw e;
	}
	trace.push(...prng.trace);
	if (k !== live.boundaries.length) return {boundary: k, what: 'the round-tripped run has fewer action boundaries'};
	if (!sameDecisions(trace, live.trace)) return {boundary: k, what: 'different decisions after the last boundary'};
	const s = snapshotOf(battle);
	if (comparable(s) !== live.final) {
		return {boundary: k, what: 'different final state', diff: firstDiff(live.sorted, sortKeys(s)) ||
			{at: '(key order of volatiles / conditions)'}};
	}
	return null;
}

// ---- main -----------------------------------------------------------------------------------

function checkScenario(file, args, featureCounts) {
	const scenario = E.readJSON(file);
	if (!scenario.turn) return null; // a team file, a lab-parity game record, a trapped-only fixture's input
	const keepNominalDraws = (scenario.description || '').includes('--keep-nominal-draws');
	const opts = {collapse: true, keepNominalDraws, mode: args.mode, roll: args.roll, setupWalks: args.setupWalks,
		setupMaxBranches: args.setupMaxBranches};
	const {snapshot} = E.buildSnapshot(scenario, path.dirname(file), opts);
	const rng = E.mulberry32(args.seed);
	const row = {scenario: path.relative(repo, file).replaceAll('\\', '/'), walks: 0, boundaries: 0, decisions: 0,
		outcomes: 0, differs: []};
	const outcomes = new Set();
	const seen = new Set();
	for (let w = 0; w < args.walks; w++) {
		const prng = new Walk(rng, args.mode, args.roll, w % 2 === 1);
		const live = liveRun(scenario, snapshot, opts, prng);
		row.walks++;
		row.boundaries += live.boundaries.length;
		row.decisions += live.trace.length;
		outcomes.add(live.outcome);
		for (const b of live.boundaries) for (const f of b.features) seen.add(f);
		const diff = roundTripRun(scenario, snapshot, opts, live);
		if (diff) row.differs.push({walk: w, choices: live.trace.map(t => t.choice), ...diff});
	}
	row.outcomes = outcomes.size;
	row.features = [...seen].sort();
	for (const f of seen) featureCounts.set(f, (featureCounts.get(f) || 0) + 1);
	return row;
}

function main() {
	const args = parseArgs(process.argv.slice(2));
	const files = scenarioFiles(args);
	const rows = [];
	const featureCounts = new Map();
	let bad = 0;
	const started = Date.now();
	for (const file of files) {
		let row;
		try {
			row = checkScenario(file, args, featureCounts);
			if (!row) continue;
			row.status = row.differs.length ? 'differs' : 'same';
		} catch (e) {
			row = {scenario: path.relative(repo, file).replaceAll('\\', '/'), status: 'error',
				error: String(e.message || e).slice(0, 300)};
		}
		if (row.status !== 'same') {
			bad++;
			console.log(JSON.stringify({scenario: row.scenario, status: row.status, error: row.error,
				differs: row.differs?.slice(0, 2)}));
		}
		rows.push(row);
	}
	const same = rows.filter(r => r.status === 'same');
	const total = key => rows.reduce((s, r) => s + (r[key] || 0), 0);
	const features = Object.fromEntries([...featureCounts.entries()].sort((a, b) => b[1] - a[1]));
	console.log(`${rows.length} scenarios: ${same.length} same, ${rows.filter(r => r.status === 'differs').length} differ, ` +
		`${rows.filter(r => r.status === 'error').length} errors; ${total('walks')} walks, ${total('boundaries')} ` +
		`round trips, ${total('decisions')} decisions, ${Math.round((Date.now() - started) / 1000)} s`);
	if (args.json) {
		fs.writeFileSync(args.json, JSON.stringify({mode: args.mode, roll: args.roll, walks: args.walks, seed: args.seed,
			shard: args.shard, scenarios: rows.length, same: same.length, features, rows}, null, 1));
	}
	process.exitCode = bad ? 1 : 0;
}

if (require.main === module) main();
module.exports = {features, comparable, snapshotOf, liveRun, roundTripRun};
