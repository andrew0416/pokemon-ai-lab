// Exact mechanics probes. These are controlled fixtures, not ladder games.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const root = path.resolve(__dirname,'..');
const {Battle,Dex,Teams,TeamValidator} = require(path.join(root,'vendor/pokemon-showdown/dist/sim'));
const {Generations,Pokemon,Move,Field,calculate} = require('@smogon/calc');
const gen=Generations.get(0);
const mon=(species,ability,moves,evs={},item='',nature='Hardy')=>({species,ability,moves,evs,item,nature,level:50});
function start(p1,p2){
 const b=new Battle({formatid:'gen9championsdoublescustomgame',seed:[1,2,3,4],
   p1:{name:'A',team:p1},p2:{name:'B',team:p2}});
 b.makeChoices('team 12','team 12'); return b;
}
const idle=()=>mon('Blissey','Natural Cure',['Splash']);
const results=[];
function test(name,fn){fn();results.push({name,status:'pass'});console.log('PASS '+name);}

test('Real M-C format is Champions doubles, validates sourced team, and brings four',()=>{
 const format='gen9championsvgc2026regmc';
 const f=Dex.formats.get(format);assert.equal(f.gameType,'doubles');assert(f.mod.startsWith('champions'));
 const team=JSON.parse(fs.readFileSync(path.join(root,'teams/psyspam-popular.json'),'utf8'));
 assert.equal(TeamValidator.get(format).validateTeam(team),null);
 const b=new Battle({formatid:format,seed:[1,2,3,4],p1:{name:'A',team:Teams.pack(team)},p2:{name:'B',team:Teams.pack(team)}});
 b.makeChoices('team 1234','team 1234');
 assert.equal(b.p1.pokemon.length,4);assert.equal(b.p2.pokemon.length,4);b.destroy();
});
test('Champions Stat Points: simulator and Smogon calculator agree',()=>{
 const set=mon('Incineroar','Intimidate',['Protect'],{hp:32,def:24,spd:10},'Chople Berry','Careful');
 const b=start([set,idle()],[idle(),idle()]); const c=new Pokemon(gen,set.species,set);
 assert.equal(b.p1.active[0].maxhp,c.maxHP());
 for(const stat of ['atk','def','spa','spd','spe']) assert.equal(b.p1.active[0].storedStats[stat],c.rawStats[stat],stat);
 b.destroy();
});
test('Armor Cannon: simulator actual damage falls in calculator rolls (no KO cap)',()=>{
 const a=mon('Armarouge','Flash Fire',['Armor Cannon'],{spa:32},'','Modest');
 const d=mon('Milotic','Marvel Scale',['Splash'],{hp:32,spd:32},'','Calm');
 const b=start([a,idle()],[d,idle()]); const hp=b.p2.active[0].hp;
 const r=calculate(gen,new Pokemon(gen,a.species,a),new Pokemon(gen,d.species,d),new Move(gen,'Armor Cannon'),new Field({gameType:'Doubles'}));
 b.makeChoices('move 1 1, move 1','move 1, move 1');
 const actual=hp-b.p2.active[0].hp; const [lo,hi]=r.range();assert(actual>=lo&&actual<=hi,`${actual} outside ${lo}..${hi}`);b.destroy();
});
test('Psychic Terrain blocks Prankster Quash but not Gravity',()=>{
 const b=start([mon('Sableye','Prankster',['Quash','Gravity']),idle()],
   [mon('Indeedee-F','Psychic Surge',['Splash']),idle()]);
 b.makeChoices('move 1 1, move 1','move 1, move 1');
 assert(b.log.some(l=>l.includes('-activate')&&l.includes('Psychic Terrain')));
 b.makeChoices('move 2, move 1','move 1, move 1');assert(b.field.getPseudoWeather('gravity'));b.destroy();
});
test('Psychic Seed Unburden persists after Grassy Terrain replaces Psychic Terrain',()=>{
 const b=start([mon('Indeedee-F','Psychic Surge',['Splash']),mon('Sneasler','Unburden',['Splash'],{},'Psychic Seed')],
   [mon('Rillaboom','Overgrow',['Grassy Terrain']),idle()]);
 const s=b.p1.active[1]; assert.equal(s.item,''); assert.equal(s.boosts.spd,1);
 const speed=s.getStat('spe');assert.equal(speed,s.storedStats.spe*2);
 b.makeChoices('move 1, move 1','move 1, move 1');assert.equal(b.field.terrain,'grassyterrain');
 assert.equal(s.getStat('spe'),speed);assert.equal(s.boosts.spd,1);b.destroy();
});
test('Wide Guard blocks terrain-boosted Expanding Force for both allies',()=>{
 const b=start([mon('Armarouge','Flash Fire',['Wide Guard']),mon('Milotic','Marvel Scale',['Splash'])],
   [mon('Armarouge','Flash Fire',['Expanding Force']),mon('Indeedee-F','Psychic Surge',['Splash'])]);
 const hp=b.p1.active.map(p=>p.hp);b.makeChoices('move 1, move 1','move 1 1, move 1');
 assert.deepEqual(b.p1.active.map(p=>p.hp),hp);b.destroy();
});
test('Flash Fire absorbs Armor Cannon',()=>{
 const b=start([mon('Armarouge','Flash Fire',['Splash']),idle()],
   [mon('Armarouge','Flash Fire',['Armor Cannon']),idle()]);
 const hp=b.p1.active[0].hp;b.makeChoices('move 1, move 1','move 1 1, move 1');
 assert.equal(b.p1.active[0].hp,hp);assert(b.p1.active[0].volatiles.flashfire);b.destroy();
});
fs.mkdirSync(path.join(root,'runs'),{recursive:true});
fs.writeFileSync(path.join(root,'runs/calibration.json'),JSON.stringify({ruleset:'champions',results},null,2));

