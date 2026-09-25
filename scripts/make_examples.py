"""Explicit fixtures: not claims about optimal spreads or the opponent's full team."""
import json
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
def save(name,obj):
    p=ROOT/name; p.parent.mkdir(parents=True,exist_ok=True)
    p.write_text(json.dumps(obj,indent=2),encoding='utf-8')
def mon(species,ability,item,moves,nature,sp):
    return dict(species=species,ability=ability,item=item,moves=moves,nature=nature,evs={k.rstrip('_'):v for k,v in sp.items()},level=50)
sableye=mon('Sableye','Prankster','Roseli Berry',['Gravity','Quash','Encore','Fake Out'],'Careful',dict(hp=32,def_=16,spd=18))
milotic=mon('Milotic','Competitive','Sitrus Berry',['Hydro Pump','Icy Wind','Hypnosis','Recover'],'Bold',dict(hp=32,def_=24,spa=10))
gardevoir=mon('Gardevoir','Trace','Gardevoirite',['Hyper Voice','Gravity','Hypnosis','Focus Blast'],'Timid',dict(hp=2,spa=32,spe=32))
charizard=mon('Charizard','Blaze','Charizardite Y',['Heat Wave','Solar Beam','Fire Blast','Protect'],'Timid',dict(hp=2,spa=32,spe=32))
flapple=mon('Flapple','Hustle','Choice Scarf',['U-turn','Grav Apple','Dragon Rush','Dual Wingbeat'],'Adamant',dict(hp=2,atk=32,spe=32))
maushold=mon('Maushold','Technician','Life Orb',['Protect','Population Bomb','Bullet Seed','Bite'],'Jolly',dict(hp=2,atk=32,spe=32))
incineroar=mon('Incineroar','Intimidate','Chople Berry',['Fake Out','Flare Blitz','Throat Chop','Parting Shot'],'Careful',dict(hp=32,def_=24,spd=10))
rillaboom=mon('Rillaboom','Grassy Surge','Miracle Seed',['Fake Out','Grassy Glide','Wood Hammer','U-turn'],'Adamant',dict(hp=32,atk=32,spd=2))
save('teams/gravity-assumed-v1.json',[sableye,milotic,gardevoir,charizard,flapple,maushold])
# Superseded assumptions above are retained for interpreting earlier smoke runs.
# Current baseline is transcribed from the user's two screenshots (2026-09-20).
sableye={**sableye,'gender':'F','nature':'Bold','evs':{'hp':32,'def':32,'spd':2}}
milotic={**milotic,'gender':'M','evs':{'hp':32,'def':32,'spd':2},'moves':['Hydro Pump','Recover','Hypnosis','Ice Beam']}
gardevoir={**gardevoir,'gender':'F','nature':'Modest','evs':{'hp':32,'def':10,'spa':24}}
charizard={**charizard,'gender':'M','nature':'Modest'}
flapple={**flapple,'gender':'M'}
maushold={**maushold,'evs':{'hp':1,'atk':32,'spe':32}}
baseline=[sableye,milotic,gardevoir,charizard,flapple,maushold]
save('teams/gravity-original.json',baseline)
save('teams/gravity-icy-wind.json',[{**p,'moves':['Hydro Pump','Recover','Hypnosis','Icy Wind']} if p['species']=='Milotic' else p for p in baseline])
save('teams/gravity-revised.json',[sableye,milotic,{**gardevoir,'moves':['Hyper Voice','Psyshock','Gravity','Protect']},incineroar,rillaboom,maushold])
save('scenarios/armor-cannon-vs-rillaboom.json',dict(ruleset='champions',attacker=mon('Armarouge','Flash Fire','Life Orb',[],'Modest',dict(hp=32,spa=32,spd=2)),defender=rillaboom,move='Armor Cannon',field=dict(gameType='Doubles')))
save('scenarios/close-combat-vs-incineroar.json',dict(ruleset='champions',attacker={**mon('Sneasler','Unburden','',[],'Adamant',dict(hp=2,atk=32,spe=32)),'boosts':{'atk':-1,'spd':1}},defender=incineroar,move='Close Combat',field=dict(gameType='Doubles',terrain='Psychic')))
save('scenarios/psyshock-vs-seed-sneasler.json',dict(ruleset='champions',attacker={**gardevoir,'species':'Gardevoir-Mega','ability':'Pixilate'},defender={**mon('Sneasler','Unburden','',[],'Adamant',dict(hp=2,atk=32,spe=32)),'boosts':{'spd':1}},move='Psyshock',field=dict(gameType='Doubles',terrain='Psychic')))
save('experiments/paired-team-screen.json',{'hypothesis':'The revised team improves the fixed psyspam matchup under the same policy; not a claim of general strength.', 'format':'gen9championsvgc2026regmc','seed':20260920,'games_per_arm':4,'policy':'vgc_myopic','opponent':'vgc_myopic','opponent_team':'teams/psyspam-popular.json','arms':{'original':'teams/gravity-original.json','revised':'teams/gravity-revised.json'},'required_confirmation':'Use unseen opponent teams, multiple policies, and team-clustered uncertainty before judging team quality.'})


