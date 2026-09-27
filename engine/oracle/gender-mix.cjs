'use strict';
// Oracle for a scenario with sets of undecided gender (board R13b): Showdown's Pokemon
// constructor draws `this.gender = genders[set.gender] || this.species.gender ||
// this.battle.sample(['M', 'F'])` (sim/pokemon.ts), a fair coin per set with neither a set gender
// nor a species' fixed one, with the battle's seeded PRNG when the players join. enumerate.cjs
// keeps that seed's draw; this script enumerates the coins instead: it runs enumerate.cjs once per
// gender assignment (the scenario with those genders written into the sets) and mixes the
// reports with weight 1/2^k. The canonical state has no gender, so the variants must reach the
// same `before` (checked); their outcomes merge by canonical state.
//
// Usage: node engine/oracle/gender-mix.cjs <scenario.json> --out <report.json> [enumerate.cjs options]
// (LAB_ROOT as for enumerate.cjs). The report has enumerate.cjs's shape plus `genderVariants`
// (the undecided sets, in p1-then-p2 team order) for strip-report.cjs.

const fs = require('node:fs');
const path = require('node:path');
const {execFileSync} = require('node:child_process');
const root = process.env.LAB_ROOT ? path.resolve(process.env.LAB_ROOT) : path.resolve(__dirname, '../..');
const {Dex} = require(path.join(root, 'vendor/pokemon-showdown/dist/sim'));
const {canonicalKey} = require('./canonical.cjs');
const {readJSON} = require('./enumerate.cjs');

function main() {
	const argv = process.argv.slice(2);
	const file = argv[0];
	const outAt = argv.indexOf('--out');
	if (!file || outAt < 0) throw new Error('usage: gender-mix.cjs <scenario.json> --out <report.json> [options]');
	const out = argv[outAt + 1];
	const rest = argv.slice(1).filter((_, i) => i + 1 !== outAt && i + 1 !== outAt + 1);
	const scenarioPath = path.resolve(file);
	const scenario = readJSON(scenarioPath);
	const dex = Dex.forFormat(scenario.format);
	const undecided = [];
	for (const side of ['p1', 'p2']) {
		const team = scenario[side].team;
		if (typeof team === 'string') throw new Error('inline teams only');
		team.forEach((set, i) => {
			if (['M', 'F', 'N'].includes(set.gender)) return;
			if (dex.species.get(set.species).gender) return;
			undecided.push({side, i, name: set.name || set.species});
		});
	}
	if (!undecided.length) throw new Error('no set of undecided gender: use enumerate.cjs');
	const variants = 2 ** undecided.length;
	const weight = 1 / variants;
	const merged = new Map();
	let first = null;
	let branches = 0;
	for (let bits = 0; bits < variants; bits++) {
		const variant = JSON.parse(JSON.stringify(scenario));
		undecided.forEach((u, k) => {
			variant[u.side].team[u.i].gender = (bits >> k) & 1 ? 'F' : 'M';
		});
		const tmp = path.join(path.dirname(scenarioPath), `.gender-mix-${process.pid}-${bits}.json`);
		const report = path.join(path.dirname(scenarioPath), `.gender-mix-${process.pid}-${bits}.report.json`);
		fs.writeFileSync(tmp, JSON.stringify(variant));
		try {
			execFileSync(process.execPath, [path.join(__dirname, 'enumerate.cjs'), tmp, '--out', report, ...rest],
				{stdio: ['ignore', 'ignore', 'inherit'], env: process.env});
			const r = readJSON(report);
			if (!first) first = r;
			else if (canonicalKey(r.before) !== canonicalKey(first.before)) {
				throw new Error(`gender variant ${bits} reaches another \`before\`: the mixture is not one position`);
			}
			branches += r.branches;
			for (const o of r.outcomes) {
				const key = canonicalKey(o.state);
				const entry = merged.get(key);
				if (entry) {
					entry.p += o.p * weight;
					entry.branches += o.branches;
				} else {
					merged.set(key, {p: o.p * weight, branches: o.branches, state: o.state, log: o.log});
				}
			}
		} finally {
			for (const f of [tmp, report]) if (fs.existsSync(f)) fs.unlinkSync(f);
		}
	}
	const outcomes = [...merged.values()].sort((a, b) => b.p - a.p);
	const result = {
		...first,
		scenario: path.relative(root, scenarioPath).replaceAll('\\', '/'),
		turn: scenario.turn,
		branches,
		distinctOutcomes: outcomes.length,
		totalProbability: outcomes.reduce((s, o) => s + o.p, 0),
		genderVariants: undecided.map(u => `${u.side}: ${u.name}`),
		outcomes,
	};
	delete result.elapsedMs;
	fs.writeFileSync(out, JSON.stringify(result, null, 1));
	console.error(`gender-mix: ${variants} variants (${undecided.length} undecided sets), ${outcomes.length} outcomes`);
}

main();
