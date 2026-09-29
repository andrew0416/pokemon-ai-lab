const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const root=path.resolve(__dirname,'../../../../..');
const oracleFile=path.join(root,'engine/oracle/enumerate.cjs');
const oracle = require(oracleFile);
const originalFile=path.join(root,'engine/oracle/scenarios/ss-redirect-tie-hidden-order.json');
const original = JSON.parse(fs.readFileSync(originalFile,'utf8'));
const hash=file=>crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
let saved=false;
const trials=[];
for (let n=1;n<=8;n++) {
 const scenario={...structuredClone(original),seed:[n,2,3,4]};
 const {snapshot,before} = oracle.buildSnapshot(structuredClone(scenario),path.dirname(originalFile),{});
 const args={mode:'full',maxBranches:500000,collapse:false,traces:true,keepNominalDraws:false};
 const result=oracle.enumerateStaged(structuredClone(scenario),snapshot,args);
 const outcomes=[...result.outcomes.values()].map(o=>({p:o.p,branches:o.branches,state:o.state,log:o.log,trace:o.trace}));
 const recipient=outcomes[0].state.sides[0].pokemon.find(m=>m.boosts?.spa===1)?.name;
 const trial={seed:scenario.seed,recipient,outcomes:outcomes.length,branches:result.branches};
 trials.push(trial);
 process.stdout.write(JSON.stringify(trial)+'\n');
 if (recipient==='Rod A' && outcomes.length===1 && outcomes[0].p===1 && !result.approximate) {
  const stem='ss-redirect-tie-hidden-order-rod-a';
  const derivedFile=path.join(__dirname,stem+'.json');
  const reportFile=path.join(__dirname,stem+'.turn.json');
  fs.writeFileSync(derivedFile,JSON.stringify(scenario,null,1)+'\n',{flag:'wx'});
  fs.writeFileSync(reportFile,JSON.stringify({scenario:'engine/benchmarks/agreement/data/contracts/'+stem+'.json',mode:'full',exact:true,showdownCommit:oracle.sourceCommit(),branches:result.branches,distinctOutcomes:outcomes.length,totalProbability:outcomes.reduce((s,o)=>s+o.p,0),before,outcomes},null,1)+'\n',{flag:'wx'});
  fs.writeFileSync(path.join(__dirname,stem+'.provenance.json'),JSON.stringify({schema:1,source_scenario:'engine/oracle/scenarios/ss-redirect-tie-hidden-order.json',source_scenario_sha256:hash(originalFile),derivation:'Only the Showdown PRNG seed is changed. The alternate setup history is independently enumerated by the pinned Showdown simulator; no Rust outputs are used.',changes:{seed:scenario.seed},trials,oracle_script_sha256:hash(oracleFile),generator_sha256:hash(__filename),showdown_commit:oracle.sourceCommit(),scenario_sha256:hash(derivedFile),report_sha256:hash(reportFile)},null,2)+'\n',{flag:'wx'});
  saved=true;break;
 }
}
if(!saved) throw new Error('No alternate deterministic oracle found within the fixed seed search bound');
