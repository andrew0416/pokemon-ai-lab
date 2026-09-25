'use strict';
// Turns an enumerate.cjs report into a compact test fixture: the starting `before` state and
// the outcome distribution (probability + canonical state), without the logs.
//
// Usage: node engine/oracle/strip-report.cjs <report.json> <fixture.json>
//
// Fixtures in engine/oracle/expected/*.turn.json are compared exactly by
// engine/scenario/tests/turn.rs. Regenerate them from a `full` report of the same scenario
// when vendor/pokemon-showdown changes.

const fs = require('node:fs');
const path = require('node:path');

function main() {
	const [input, output] = process.argv.slice(2);
	if (!input || !output) throw new Error('usage: strip-report.cjs <report.json> <fixture.json>');
	const report = JSON.parse(fs.readFileSync(input, 'utf8'));
	if (report.mode !== 'full' || !report.exact) throw new Error('only exact (full) reports make fixtures');
	// The report's path is relative to wherever enumerate.cjs ran; scenarios live in one place.
	const fixture = {
		scenario: `engine/oracle/scenarios/${path.basename(report.scenario)}`,
		format: report.format,
		turn: report.turn,
		mode: report.mode,
		showdownCommit: report.showdownCommit,
		distinctOutcomes: report.distinctOutcomes,
		before: report.before,
		outcomes: report.outcomes.map(o => ({p: o.p, state: o.state})),
	};
	fs.writeFileSync(output, JSON.stringify(fixture) + '\n');
	console.log(`wrote ${output}: ${fixture.outcomes.length} outcomes`);
}

main();
