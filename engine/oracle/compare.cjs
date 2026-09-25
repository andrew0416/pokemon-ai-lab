'use strict';
// Compares two outcome distributions (oracle reports, or an oracle report and a lab-engine
// report) by canonical state. Prints the total variation distance and the largest gaps.
//
// Usage: node engine/oracle/compare.cjs <a.json> <b.json> [--top N]
//
// When one side is Monte Carlo, a TV distance near the sampling noise is expected: with n
// samples and k outcomes it is roughly sqrt(k / (2 * pi * n)).

const fs = require('node:fs');
const {canonicalKey} = require('./canonical.cjs');

function load(file) {
	const report = JSON.parse(fs.readFileSync(file, 'utf8'));
	const dist = new Map();
	for (const o of report.outcomes) dist.set(canonicalKey(o.state), {p: o.p, o});
	return {report, dist};
}

function compare(a, b) {
	const keys = new Set([...a.dist.keys(), ...b.dist.keys()]);
	let tv = 0;
	const gaps = [];
	for (const k of keys) {
		const pa = a.dist.get(k)?.p ?? 0;
		const pb = b.dist.get(k)?.p ?? 0;
		tv += Math.abs(pa - pb) / 2;
		gaps.push({pa, pb, gap: Math.abs(pa - pb), o: (a.dist.get(k) ?? b.dist.get(k)).o});
	}
	gaps.sort((x, y) => y.gap - x.gap);
	const onlyA = [...a.dist.keys()].filter(k => !b.dist.has(k)).length;
	const onlyB = [...b.dist.keys()].filter(k => !a.dist.has(k)).length;
	return {tv, onlyA, onlyB, gaps};
}

function main() {
	const argv = process.argv.slice(2);
	const top = argv.includes('--top') ? Number(argv[argv.indexOf('--top') + 1]) : 5;
	const [fa, fb] = argv.filter((x, i) => !x.startsWith('--') && argv[i - 1] !== '--top');
	const a = load(fa);
	const b = load(fb);
	const r = compare(a, b);
	const mc = [a.report, b.report].find(x => x.mode === 'mc');
	const noise = mc ? Math.sqrt(r.gaps.length / (2 * Math.PI * mc.branches)) : 0;
	console.log(`A: ${fa} (${a.report.mode}, ${a.dist.size} outcomes)`);
	console.log(`B: ${fb} (${b.report.mode}, ${b.dist.size} outcomes)`);
	console.log(`TV distance ${r.tv.toFixed(6)}` + (mc ? ` (Monte Carlo noise ~${noise.toFixed(4)})` : ''));
	console.log(`outcomes only in A: ${r.onlyA}, only in B: ${r.onlyB}`);
	for (const g of r.gaps.slice(0, top)) {
		if (g.gap === 0) break;
		console.log(`  A ${g.pa.toFixed(5)}  B ${g.pb.toFixed(5)}  ${g.o.log.filter(l => /\|-(damage|status|crit|miss|fail)|\|cant/.test(l)).join(' ; ').slice(0, 300)}`);
	}
	process.exitCode = r.tv > Math.max(1e-9, 3 * noise) ? 1 : 0;
}

module.exports = {compare};
if (require.main === module) main();
