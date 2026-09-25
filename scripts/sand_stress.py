import os,sys,json,hashlib,itertools
from pathlib import Path
sys.path.insert(0,'D:/pokemon-ai-lab/scripts')
import lab
lab.configure()
from vgc.rl.env import SimWorker,DirectBattle
OUT=Path(sys.argv[1]);OUT.mkdir(parents=True,exist_ok=True)
seeds=list(range(101,133))
opponents={
 'double_rock':'move rockslide mega, move rockslide',
 'focus_milotic':'move knockoff mega 2, move highhorsepower 2',
 'rock_iron':'move rockslide mega, move ironhead 2',
 'protect_tar':'move protect mega, move ironhead 2',
 'protect_exc':'move knockoff mega 2, move protect',
 'split_pressure':'move knockoff mega 1, move ironhead 2',
 'intimidate_switch':'switch 3, move highhorsepower 2',
}
plans={
 'gravity_sleep_exc':('team 1235','move gravity, move hypnosis 2'),
 'quash_sleep_exc':('team 1235','move quash 2, move hypnosis 2'),
 'gravity_hydro_exc':('team 1235','move gravity, move hydropump 2'),
 'sun_sleep_exc':('team 1235','move sunnyday, move hypnosis 2'),
 'partner_lead':('team 1325',None),
}
def snapshot(b):
 return {side:{'hp':sum(p.current_hp_fraction for p in b.battles[side].team.values()),'fainted':sum(p.fainted for p in b.battles[side].team.values()),'active':[{'species':p.species,'hp':p.current_hp_fraction,'status':str(p.status)} if p else None for p in b.battles[side].active_pokemon]} for side in ('p1','p2')}
def run():
 worker=SimWorker(lab.SHOWDOWN);rows=[]
 enemy=lab.node_tool('team',lab.ROOT/'teams/library/doubles/m-c/sand-owen/team.json',lab.FORMAT)
 try:
  for variant in ('starmie','gardevoir'):
   own=lab.node_tool('team',lab.ROOT/f'teams/taunt-panel/own-{variant}.json',lab.FORMAT)
   for plan,(preview,action) in plans.items():
    action=action or ('move quash 2, move liquidation mega 2' if variant=='starmie' else 'move gravity, move hypnosis mega 2')
    for opp,opp_action in opponents.items():
     for seed in seeds:
      key=f'{variant}-{plan}-{opp}-{seed}'
      b=DirectBattle.start(worker,key,own,enemy,battle_format=lab.FORMAT,seed=[9182,7431,5227,seed])
      try:
       b.step({'p1':preview,'p2':'team 5612'})
       r=b.step({'p1':action,'p2':opp_action})
       row={'id':key,'variant':variant,'plan':plan,'opponent':opp,'seed':seed,'choices':{'p1':action,'p2':opp_action},'state':snapshot(b),'log':r.lines['p1'],'completed_battle':b.ended}
       rows.append(row)
      finally:b.close()
   print(variant,len(rows),flush=True)
 finally:worker.close()
 (OUT/'branches.json').write_text(json.dumps(rows,ensure_ascii=False,indent=2),encoding='utf8')
 summary=[]
 for (v,p,o),group in itertools.groupby(rows,key=lambda r:(r['variant'],r['plan'],r['opponent'])):
  g=list(group);n=len(g)
  summary.append({'variant':v,'plan':p,'opponent':o,'n':n,'exc_sleep':sum(any(x and x['species']=='excadrill' and 'SLP' in x['status'] for x in r['state']['p2']['active']) for r in g),'our_any_ko':sum(r['state']['p1']['fainted']>0 for r in g),'our_slot2_mean_hp':round(sum(r['state']['p1']['active'][1]['hp'] if r['state']['p1']['active'][1] else 0 for r in g)/n,3),'exc_mean_hp':round(sum(next((x['hp'] for x in r['state']['p2']['active'] if x and x['species']=='excadrill'),0) for r in g)/n,3)})
 (OUT/'summary.json').write_text(json.dumps(summary,indent=2),encoding='utf8')
 manifest={'format':lab.FORMAT,'seeds':seeds,'prefix':[9182,7431,5227],'forced_enemy_preview':'team 5612 (Tyranitar Excadrill; Salamence Indeedee)','plans':plans,'opponents':opponents,'scope':'Turn-one scenario grid, not completed games or win rates','hashes':{str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in [lab.ROOT/'teams/taunt-panel/own-starmie.json',lab.ROOT/'teams/taunt-panel/own-gardevoir.json',lab.ROOT/'teams/library/doubles/m-c/sand-owen/team.json']}}
 (OUT/'manifest.json').write_text(json.dumps(manifest,indent=2),encoding='utf8')
run()
