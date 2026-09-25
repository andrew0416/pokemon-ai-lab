'use strict';
// Showdown oracle for the initial position: every outcome of team preview + battle start
// (lead switch-in effects such as Trace, weather and terrain abilities) with its exact
// probability, plus the `before` state enumerate.cjs uses (the scenario's fixed seed).
//
// Usage:
//   node engine/oracle/initial.cjs <scenario.json> [--out file]
//
// Only scenarios without `setupTurns`/`patch` are accepted: those change the position after
// the start and are not part of the initial distribution.

const fs = require('node:fs');
const path = require('node:path');
const root = path.resolve(__dirname, '../..');
const {Battle, Teams} = require(path.join(root, 'vendor/pokemon-showdown/dist/sim'));
const {canonical, canonicalKey} = require('./canonical.cjs');
const {buildSnapshot, ScriptedPRNG, nextPrefix, loadTeam, readJSON, sourceCommit} = require('./enumerate.cjs');

function parseArgs(argv) {
	const args = {file: null, out: null};
	for (let i = 0; i < argv.length; i++) {
		if (argv[i] === '--out') args.out = argv[++i];
		else if (!args.file) args.file = argv[i];
		else throw new Error(`unexpected argument ${argv[i]}`);
	}
	if (!args.file) throw new Error('usage: initial.cjs <scenario.json> [--out file]');
	return args;
}

function startBattle(scenario, baseDir, prng) {
	const battle = new Battle({
		formatid: scenario.format,
		seed: scenario.seed || 'sodium,00000000000000000000000000000000',
		strictChoices: true,
	});
	battle.setPlayer('p1', {name: 'p1', team: Teams.pack(loadTeam(scenario.p1.team, baseDir))});
	battle.setPlayer('p2', {name: 'p2', team: Teams.pack(loadTeam(scenario.p2.team, baseDir))});
	if (battle.requestState !== 'teampreview') throw new Error('expected team preview');
	// Everything random from here on (start order ties, Trace, ...) goes through the script.
	battle.prng = prng;
	battle.makeChoices(`team ${scenario.p1.order || '123456'}`, `team ${scenario.p2.order || '123456'}`);
	if (battle.ended) throw new Error('battle ended during start');
	return battle;
}

function main() {
	const args = parseArgs(process.argv.slice(2));
	const scenarioPath = path.resolve(args.file);
	const scenario = readJSON(scenarioPath);
	const baseDir = path.dirname(scenarioPath);
	if ((scenario.setupTurns || []).length || Object.keys(scenario.patch || {}).length) {
		throw new Error('setupTurns/patch are not part of the initial distribution');
	}
	const outcomes = new Map();
	let prefix = [];
	let branches = 0;
	while (prefix) {
		branches++;
		const prng = new ScriptedPRNG(prefix, 'full');
		const state = canonical(startBattle(scenario, baseDir, prng));
		const p = prng.trace.reduce((acc, t) => acc * t.weights[t.choice], 1);
		const key = canonicalKey(state);
		const entry = outcomes.get(key);
		if (entry) {
			entry.p += p;
			entry.branches++;
		} else {
			outcomes.set(key, {p, branches: 1, state});
		}
		prefix = nextPrefix(prng.trace);
	}
	const sorted = [...outcomes.values()].sort((a, b) => b.p - a.p ||
		(canonicalKey(a.state) < canonicalKey(b.state) ? -1 : 1));
	const report = {
		provenance: 'engine/oracle/initial.cjs',
		scenario: path.relative(root, scenarioPath).replaceAll('\\', '/'),
		format: scenario.format,
		showdownCommit: sourceCommit(),
		branches,
		distinctOutcomes: sorted.length,
		totalProbability: sorted.reduce((s, o) => s + o.p, 0),
		before: buildSnapshot(scenario, baseDir).before,
		outcomes: sorted,
	};
	const text = JSON.stringify(report, null, 1) + '\n';
	if (args.out) {
		fs.mkdirSync(path.dirname(path.resolve(args.out)), {recursive: true});
		fs.writeFileSync(args.out, text);
	}
	else process.stdout.write(text);
	console.error(`initial: ${branches} branches -> ${sorted.length} outcomes`);
}

main();
