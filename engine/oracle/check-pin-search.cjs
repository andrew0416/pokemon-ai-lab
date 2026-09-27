'use strict';
// V3 (lead-turn trace): checks the oracle's staged search for pinned setup turns (enumerate.cjs
// `findPinnedStaged`) on recorded positions. For every position given, the recorded trace of its
// last pinned setup turn is dropped and random walks are turned off, so that turn can only be found
// by the staged search; the search must find a branch that replays to the pin (checked inside
// `findPinned`) within the cap. Earlier setup turns replay their recorded traces as usual.
// Prints one line per position that was not found and a summary; `--json` writes every row (runs,
// states expanded, seconds).
//
// Usage: LAB_ROOT=<checkout with vendor/> node engine/oracle/check-pin-search.cjs <position.json>...
//            [--dir <positions dir>]... [--every K] [--cap N] [--json <out.json>]
// `--every K` takes every K-th position of each directory (sorted by name).

const fs = require('node:fs');
const path = require('node:path');
const {buildSnapshot, readJSON} = require('./enumerate.cjs');

function parseArgs(argv) {
	const args = {files: [], dirs: [], every: 1, cap: 200000, json: null};
	for (let i = 0; i < argv.length; i++) {
		const a = argv[i];
		if (a === '--dir') args.dirs.push(argv[++i]);
		else if (a === '--every') args.every = Number(argv[++i]);
		else if (a === '--cap') args.cap = Number(argv[++i]);
		else if (a === '--json') args.json = argv[++i];
		else args.files.push(a);
	}
	for (const d of args.dirs) {
		const names = fs.readdirSync(d).filter(f => /\.s\d+\.json$/.test(f)).sort();
		names.filter((f, i) => i % args.every === 0).forEach(f => args.files.push(path.join(d, f)));
	}
	return args;
}

function main() {
	const args = parseArgs(process.argv.slice(2));
	const rows = [];
	for (const file of args.files) {
		const scenario = readJSON(file);
		const pins = scenario.setupStates || [];
		const k = pins.length - 1;
		const row = {position: path.basename(file), setupTurns: pins.length};
		if (k < 0 || !pins[k]) {
			row.status = 'no pinned setup turn';
			rows.push(row);
			continue;
		}
		const baseDir = path.dirname(path.resolve(file));
		// The start and the earlier setup turns as usual (recorded traces, else walks and searches),
		// so that only the last setup turn is left to the staged search.
		let earlier;
		try {
			earlier = buildSnapshot({...scenario, setupTurns: scenario.setupTurns.slice(0, k), setupStates: pins.slice(0, k),
				setupTraces: (scenario.setupTraces || []).slice(0, k)}, baseDir, {collapse: true}).traces;
		} catch (e) {
			row.status = 'earlier setup failed';
			row.error = String(e.message).slice(0, 300);
			console.log(JSON.stringify(row));
			rows.push(row);
			continue;
		}
		scenario.startTrace = earlier.start;
		scenario.setupTraces = [...earlier.setup, null];
		const started = Date.now();
		try {
			const {traces: found} = buildSnapshot(scenario, baseDir, {
				collapse: true, setupWalks: 0, setupStagedMaxBranches: args.cap, setupMaxBranches: 0,
			});
			row.how = found.search[k];
			row.status = /^staged search/.test(row.how) ? 'found' : `found by ${row.how}`;
		} catch (e) {
			row.status = /pinned state/.test(e.message) ? 'not found' : 'error';
			row.error = String(e.message).slice(0, 300);
		}
		row.seconds = (Date.now() - started) / 1000;
		if (row.status !== 'found') console.log(JSON.stringify(row));
		rows.push(row);
	}
	const count = s => rows.filter(r => r.status === s).length;
	const found = rows.filter(r => r.status === 'found');
	const runs = found.map(r => Number((r.how.match(/\((\d+) runs/) || [])[1])).sort((a, b) => a - b);
	const median = runs.length ? runs[Math.floor(runs.length / 2)] : null;
	console.log(`${rows.length} positions: ${count('found')} found by the staged search, ${count('not found')} not found, ` +
		`${count('error')} errors, ${count('no pinned setup turn')} without a pinned setup turn, ` +
		`${count('earlier setup failed')} whose earlier setup failed; runs median ${median}, ` +
		`max ${runs.length ? runs[runs.length - 1] : null}`);
	if (args.json) fs.writeFileSync(args.json, JSON.stringify(rows, null, 1));
	process.exitCode = count('not found') || count('error') ? 1 : 0;
}

main();
