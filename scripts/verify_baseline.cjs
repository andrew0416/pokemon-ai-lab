const fs=require('node:fs');
const path=require('node:path');
const assert=require('node:assert/strict');
const root=path.resolve(__dirname,'..');
const {Battle,TeamValidator}=require(path.join(root,'vendor/pokemon-showdown/dist/sim'));
const {Generations,Pokemon}=require('@smogon/calc');
const team=JSON.parse(fs.readFileSync(path.join(root,'teams/gravity-original.json'),'utf8'));
const source=JSON.parse(fs.readFileSync(path.join(root,'teams/gravity-original.provenance.json'),'utf8'));
assert.equal(TeamValidator.get('gen9championsvgc2026regmc').validateTeam(team),null);
const b=new Battle({formatid:'gen9championsdoublescustomgame',seed:[1,2,3,4],p1:{name:'A',team},p2:{name:'B',team}});
try {
 for(let i=0;i<team.length;i++){
  const set=team[i], sim=b.p1.pokemon[i], calc=new Pokemon(Generations.get(0),set.species,set);
  const actual=[sim.maxhp,...source.stat_order.slice(1).map(s=>sim.storedStats[s])];
  assert.deepEqual(actual,source.expected_stats[set.species],set.species+' Showdown');
  assert.deepEqual(source.stat_order.map(s=>calc.rawStats[s]),source.expected_stats[set.species],set.species+' calculator');
 }
 const report={status:'pass',pokemon:6,stats_per_engine:36,engines:['Showdown','Smogon calc'],source:source.source};
 fs.writeFileSync(path.join(root,'runs/screenshot-baseline-check.json'),JSON.stringify(report,null,2));
 console.log(JSON.stringify(report,null,2));
} finally {b.destroy();}
