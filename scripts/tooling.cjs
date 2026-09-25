'use strict';
const fs = require('node:fs');
const path = require('node:path');
const root = path.resolve(__dirname, '..');
const {Teams, TeamValidator, Dex} = require(path.join(root, 'vendor/pokemon-showdown/dist/sim'));
const calc = require('@smogon/calc');
const [cmd, input, format = 'gen9championsvgc2026regmc'] = process.argv.slice(2);
function read(file) { return JSON.parse(fs.readFileSync(file, 'utf8').replace(/^\uFEFF/, '')); }
if (cmd === 'import-ots') {
  const provenance = read(input);
  const dir = path.dirname(input);
  const team = Teams.import(fs.readFileSync(path.join(dir, provenance.source_file), 'utf8'));
  for (const set of team) {
    set.level = provenance.simulation_assumptions.level;
    set.evs = provenance.simulation_assumptions[set.species];
    if (!set.evs) throw new Error('Missing explicit SP assumption: ' + set.species);
  }
  fs.writeFileSync(path.join(dir, provenance.simulation_file), JSON.stringify(team, null, 2));
  console.log('Imported OTS with separately recorded SP assumptions.');
} else if (cmd === 'team') {
  const text = fs.readFileSync(input, 'utf8').replace(/^\uFEFF/, '');
  const team = input.endsWith('.json') ? read(input) : Teams.import(text);
  if (!team?.length) throw new Error('Empty/unreadable team');
  const errors = TeamValidator.get(format).validateTeam(team);
  if (errors?.length) { console.error(errors.join('\n')); process.exit(2); }
  console.log(Teams.pack(team));
} else if (cmd === 'calc') {
  const spec = read(input);
  if (!['champions', 'gen9'].includes(spec.ruleset)) throw new Error('ruleset must be champions or gen9');
  const gen = calc.Generations.get(spec.ruleset === 'champions' ? 0 : 9);
  const attacker = new calc.Pokemon(gen, spec.attacker.species, spec.attacker);
  const defender = new calc.Pokemon(gen, spec.defender.species, spec.defender);
  const move = new calc.Move(gen, spec.move, spec.moveOptions || {});
  const result = calc.calculate(gen, attacker, defender, move, new calc.Field(spec.field || {}));
  const range = result.range();
  console.log(JSON.stringify({ruleset: spec.ruleset, calcVersion: require('@smogon/calc/package.json').version,
    range, defenderHP: defender.maxHP(), percent: range.map(n=>Math.round(n/defender.maxHP()*1000)/10),
    damage: result.damage, attackerStats: attacker.rawStats, defenderStats: defender.rawStats,
    description: result.desc(), note: 'Damage conditional on hitting; not a full-turn prediction.'}, null, 2));
} else if (cmd === 'formats') {
  console.log(JSON.stringify(Dex.formats.all().filter(f=>f.mod?.startsWith('champions')).map(f=>({id:f.id,name:f.name,gameType:f.gameType||'singles'})), null, 2));
} else { throw new Error('Usage: tooling.cjs team|calc|formats [file] [format]'); }
