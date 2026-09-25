'use strict';
// Showdown oracle for trapping: after a scenario's team preview, setup turns and patch, which
// active Pokémon Showdown marks `trapped` (set by `endTurn` through `runEvent('TrapPokemon')`;
// `side.chooseSwitch` rejects a switch of a trapped Pokémon in a move request), so the engine's
// `turn::trapped` can be compared.
//
// Usage: node engine/oracle/trapped.cjs <scenario.json> --out <file>

const fs = require('node:fs');
const path = require('node:path');
const {buildSnapshot, readJSON} = require('./enumerate.cjs');
const root = process.env.LAB_ROOT ? path.resolve(process.env.LAB_ROOT) : path.resolve(__dirname, '../..');
const {Battle} = require(path.join(root, 'vendor/pokemon-showdown/dist/sim'));

function main() {
	const argv = process.argv.slice(2);
	const file = argv[0];
	const outIndex = argv.indexOf('--out');
	if (!file || outIndex < 0) throw new Error('usage: trapped.cjs <scenario.json> --out <file>');
	const scenario = readJSON(file);
	const {snapshot, before} = buildSnapshot(scenario, path.dirname(path.resolve(file)));
	const battle = Battle.fromJSON(snapshot);
	const trapped = {};
	for (const sideId of ['p1', 'p2']) {
		trapped[sideId] = {};
		for (const mon of battle[sideId].active) {
			if (mon && !mon.fainted) trapped[sideId][mon.name] = !!mon.trapped;
		}
	}
	const report = {scenario: path.basename(file), before, trapped};
	fs.writeFileSync(argv[outIndex + 1], JSON.stringify(report, null, 1) + '\n');
	console.log(JSON.stringify(trapped));
}

main();
