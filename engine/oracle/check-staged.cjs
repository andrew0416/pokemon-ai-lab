'use strict';
// Checks the staged enumeration (enumerate.cjs --staged, JJ-heavy-turn-parity) against the oracle
// fixtures: for every `expected/<name>.turn.json` (full) and `expected/<name>.extremes.json`, the
// staged enumeration of its scenario in the fixture's mode must reach the same `before` state and
// the same canonical outcomes with the same probabilities (tolerance 1e-9) as the fixture, which
// the plain enumeration made. Prints one line per fixture that differs and a summary; the exit
// code is 1 if any differs.
//
// Usage: LAB_ROOT=<checkout with vendor/> node engine/oracle/check-staged.cjs [--shard i/n]
//            [--only <substring>] [--max-branches N] [--json <out.json>]

const fs = require('node:fs');
const path = require('node:path');
const {buildSnapshot, enumerateStaged, readJSON} = require('./enumerate.cjs');
const {canonicalKey} = require('./canonical.cjs');

const repo = path.resolve(__dirname, '../..');

function parseArgs(argv) {
	const args = {shard: [0, 1], only: null, maxBranches: 2000000, json: null};
	for (let i = 0; i < argv.length; i++) {
		const a = argv[i];
		if (a === '--shard') args.shard = argv[++i].split('/').map(Number);
		else if (a === '--only') args.only = argv[++i];
		else if (a === '--max-branches') args.maxBranches = Number(argv[++i]);
		else if (a === '--json') args.json = argv[++i];
		else throw new Error(`unexpected argument ${a}`);
	}
	return args;
}

function distribution(outcomes) {
	const d = new Map();
	for (const o of outcomes) {
		const k = canonicalKey(o.state);
		d.set(k, (d.get(k) || 0) + o.p);
	}
	return d;
}

function main() {
	const args = parseArgs(process.argv.slice(2));
	const dir = path.join(__dirname, 'expected');
	const files = fs.readdirSync(dir)
		.filter(f => f.endsWith('.turn.json') || f.endsWith('.extremes.json'))
		.filter(f => !args.only || f.includes(args.only))
		.sort()
		.filter((f, i) => i % args.shard[1] === args.shard[0]);
	const rows = [];
	let bad = 0;
	for (const f of files) {
		const fixture = readJSON(path.join(dir, f));
		if (fixture.staged) continue; // made by the staged enumeration itself: nothing to check against
		const scenarioPath = path.resolve(repo, fixture.scenario);
		const row = {fixture: f, mode: fixture.mode, plainBranches: fixture.branches};
		try {
			const scenario = readJSON(scenarioPath);
			// A fixture made with --keep-nominal-draws says so in its scenario's description.
			const keepNominalDraws = (scenario.description || '').includes('--keep-nominal-draws');
			const opts = {collapse: true, keepNominalDraws};
			const {snapshot, before} = buildSnapshot(scenario, path.dirname(scenarioPath), opts);
			const started = Date.now();
			const result = enumerateStaged(scenario, snapshot,
				{...opts, mode: fixture.mode, maxBranches: args.maxBranches});
			row.ms = Date.now() - started;
			row.stagedBranches = result.branches;
			row.stages = result.staged.stages;
			row.maxFrontier = result.staged.maxFrontier;
			const want = distribution(fixture.outcomes);
			const got = distribution([...result.outcomes.values()]);
			let maxDiff = 0;
			let onlyFixture = 0;
			let onlyStaged = 0;
			for (const [k, p] of want) {
				if (!got.has(k)) onlyFixture++;
				else maxDiff = Math.max(maxDiff, Math.abs(p - got.get(k)));
			}
			for (const k of got.keys()) if (!want.has(k)) onlyStaged++;
			const sameBefore = canonicalKey(before) === canonicalKey(fixture.before);
			row.status = sameBefore && !onlyFixture && !onlyStaged && maxDiff <= 1e-9 ? 'same' : 'differs';
			Object.assign(row, {sameBefore, onlyFixture, onlyStaged, maxDiff, outcomes: got.size});
		} catch (e) {
			row.status = 'error';
			row.error = String(e.message || e).slice(0, 300);
		}
		if (row.status !== 'same') {
			bad++;
			console.log(JSON.stringify(row));
		}
		rows.push(row);
	}
	const same = rows.filter(r => r.status === 'same');
	const plain = same.reduce((s, r) => s + (r.plainBranches || 0), 0);
	const staged = same.reduce((s, r) => s + r.stagedBranches, 0);
	console.log(`${rows.length} fixtures: ${same.length} same, ${rows.filter(r => r.status === 'differs').length} differ, ` +
		`${rows.filter(r => r.status === 'error').length} errors; runs ${staged} staged vs ${plain} plain`);
	if (args.json) fs.writeFileSync(args.json, JSON.stringify(rows, null, 1));
	process.exitCode = bad ? 1 : 0;
}

main();
