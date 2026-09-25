// Read-only integrity/legality verification. Usage: node verify-team-library.cjs [library root]
const fs=require('fs'),path=require('path');
const project=path.resolve(__dirname,'..');
const sim=fs.existsSync(path.join(project,'vendor/pokemon-showdown/dist/sim'))?path.join(project,'vendor/pokemon-showdown/dist/sim'):'D:/pokemon-ai-lab/vendor/pokemon-showdown/dist/sim';
const {TeamValidator}=require(sim);
const root=path.resolve(process.argv[2]||path.join(project,'teams/library'));
const read=p=>JSON.parse(fs.readFileSync(p,'utf8').replace(/^\uFEFF/,''));
const index=read(path.join(root,'index.json'));let ready=0,quarantined=0;
for(const entry of index.teams){
 const dir=path.join(root,entry.path),meta=read(path.join(dir,'metadata.json')),team=read(path.join(dir,'team.json'));
 if(team.length!==6||team.some(s=>s.moves.length!==4))throw Error('Incomplete team: '+entry.id);
 if(!fs.existsSync(path.join(dir,entry.format==='singles'?'source-cards.json':'source.txt')))throw Error('Missing source: '+entry.id);
 const errors=TeamValidator.get(meta.validation_format).validateTeam(structuredClone(team))||[];
 if(JSON.stringify(errors)!==JSON.stringify(meta.validation_errors||[]))throw Error('Validation changed: '+entry.id+JSON.stringify(errors));
 if(entry.simulation_ready&&(errors.length||!meta.sp_public))throw Error('Incorrect ready flag: '+entry.id);
 if(entry.simulation_ready)ready++;
 if(errors.length)quarantined++;
}
console.log(JSON.stringify({verified:index.teams.length,simulation_ready:ready,quarantined,summary:index.summary},null,2));
