'use strict';
// Compares two outcome distributions feature by feature (marginals), for turns whose joint
// distribution is too large for a Monte Carlo sample to test directly (spread moves: most
// sampled outcomes are seen once, so the joint TV distance is mostly sampling noise).
//
// A feature is one leaf of the canonical state (a Pokémon's hp, status, a volatile, the
// field's weather duration, ...). For each feature it prints the TV distance between the two
// marginals and the Monte Carlo noise level sqrt(k / (2 * pi * n)); a feature is flagged when
// its distance exceeds 4x that noise (or 1e-9 when neither side is Monte Carlo).
//
// Usage: node engine/oracle/marginals.cjs <a.json> <b.json>

const fs = require('node:fs');

function leaves(value, path, out) {
	if (value && typeof value === 'object' && !Array.isArray(value)) {
		for (const [k, v] of Object.entries(value)) leaves(v, path ? `${path}.${k}` : k, out);
		// Volatile/condition maps: presence itself is a feature.
		if (/(volatiles|conditions|pseudoWeather)$/.test(path)) out[`${path}#keys`] = Object.keys(value).sort().join(',');
		return;
	}
	if (Array.isArray(value)) {
		value.forEach((v, i) => leaves(v, `${path}[${i}]`, out));
		return;
	}
	out[path] = JSON.stringify(value);
}

// Pokémon are keyed by name so the path does not depend on sort position.
function features(state) {
	const s = JSON.parse(JSON.stringify(state));
	for (const side of s.sides) {
		const byName = {};
		for (const p of side.pokemon) byName[p.name] = p;
		side.pokemon = byName;
	}
	const out = {};
	leaves(s, '', out);
	return out;
}

function marginals(report) {
	const m = new Map();
	for (const o of report.outcomes) {
		for (const [path, v] of Object.entries(features(o.state))) {
			if (!m.has(path)) m.set(path, new Map());
			const d = m.get(path);
			d.set(v, (d.get(v) || 0) + o.p);
		}
	}
	return m;
}

function main() {
	const [fa, fb] = process.argv.slice(2);
	const a = JSON.parse(fs.readFileSync(fa, 'utf8'));
	const b = JSON.parse(fs.readFileSync(fb, 'utf8'));
	const ma = marginals(a);
	const mb = marginals(b);
	const mc = [a, b].find(r => r.mode === 'mc');
	let worst = 0;
	let flagged = 0;
	const rows = [];
	for (const path of new Set([...ma.keys(), ...mb.keys()])) {
		const da = ma.get(path) || new Map();
		const db = mb.get(path) || new Map();
		const values = new Set([...da.keys(), ...db.keys()]);
		let tv = 0;
		for (const v of values) tv += Math.abs((da.get(v) || 0) - (db.get(v) || 0)) / 2;
		const noise = mc ? Math.sqrt(values.size / (2 * Math.PI * mc.branches)) : 0;
		const bad = tv > Math.max(1e-9, 4 * noise);
		if (bad) flagged++;
		worst = Math.max(worst, noise ? tv / noise : tv);
		rows.push({path, k: values.size, tv, noise, bad});
	}
	rows.sort((x, y) => (y.noise ? y.tv / y.noise : y.tv) - (x.noise ? x.tv / x.noise : x.tv));
	console.log(`A: ${fa} (${a.mode}, ${a.outcomes.length} outcomes)`);
	console.log(`B: ${fb} (${b.mode}, ${b.outcomes.length} outcomes)`);
	console.log(`${rows.length} features, ${flagged} flagged (TV > 4x noise)`);
	for (const r of rows.slice(0, 15)) {
		console.log(`  ${r.bad ? '!!' : '  '} ${r.path}  values ${r.k}  TV ${r.tv.toFixed(5)}  noise ${r.noise.toFixed(5)}`);
	}
	process.exitCode = flagged ? 1 : 0;
}

main();
