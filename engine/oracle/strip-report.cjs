'use strict';
// Turns an enumerate.cjs report into a compact test fixture: the starting `before` state and
// the outcome distribution (probability + canonical state), without the logs.
//
// Usage: node engine/oracle/strip-report.cjs <report.json> <fixture.json>
//
// Fixtures in engine/oracle/expected/*.turn.json are compared exactly by
// engine/scenario/tests/turn.rs. Regenerate them from a `full` report of the same scenario
// when vendor/pokemon-showdown changes. A `mc` report (turns Showdown cannot enumerate, e.g.
// multi-hit moves) makes a *.mc.json fixture: its `branches` is the sample count and the
// engine's exact distribution is compared within sampling noise (`common::assert_mc_parity`).
// An `extremes` report (damage rolls only min and max, 1/2 each) makes a *.extremes.json
// fixture, compared exactly against the engine's `RollMode::Extremes`
// (`common::assert_extremes_parity`, WORKPLAN F18).

const fs = require('node:fs');
const path = require('node:path');

function main() {
	const [input, output] = process.argv.slice(2);
	if (!input || !output) throw new Error('usage: strip-report.cjs <report.json> <fixture.json>');
	const report = JSON.parse(fs.readFileSync(input, 'utf8'));
	const wanted = output.endsWith('.mc.json') ? 'mc' : output.endsWith('.extremes.json') ? 'extremes' : 'full';
	const ok = wanted === 'full' ? (report.mode === 'full' && report.exact) : report.mode === wanted;
	if (!ok) {
		throw new Error({
			mc: 'a *.mc.json fixture needs a --mode mc report',
			extremes: 'a *.extremes.json fixture needs a --mode extremes report',
			full: 'only exact (full) reports make *.turn.json fixtures',
		}[wanted]);
	}
	// The report's path is relative to wherever enumerate.cjs ran; scenarios live in one place.
	const fixture = {
		scenario: `engine/oracle/scenarios/${path.basename(report.scenario)}`,
		format: report.format,
		turn: report.turn,
		mode: report.mode,
		branches: report.branches,
		showdownCommit: report.showdownCommit,
		distinctOutcomes: report.distinctOutcomes,
		before: report.before,
		outcomes: report.outcomes.map(o => ({p: o.p, state: o.state})),
	};
	fs.writeFileSync(output, JSON.stringify(fixture) + '\n');
	console.log(`wrote ${output}: ${fixture.outcomes.length} outcomes`);
}

main();
