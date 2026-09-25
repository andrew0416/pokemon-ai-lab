"""Local doubles laboratory. No accounts or public ladder entry points."""
from __future__ import annotations
import argparse
import hashlib
import importlib.metadata
import json
import os
import random
import shutil
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
VENDOR = ROOT / 'vendor' / 'pokemon-vgc-ai'
SHOWDOWN = ROOT / 'vendor' / 'pokemon-showdown'
FORMAT = 'gen9championsvgc2026regmc'
NODE = shutil.which('node')
if not sys.flags.utf8_mode or os.environ.get('PYTHONHASHSEED') != '0':
    os.environ['PYTHONHASHSEED']='0'
    raise SystemExit(subprocess.call([sys.executable, '-X', 'utf8', *sys.argv]))
os.environ['PYTHONUTF8'] = '1'
os.environ['PYTHONIOENCODING'] = 'utf-8'
if NODE:
    os.environ['VGC_NODE'] = NODE

def dump(path, obj):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(obj, ensure_ascii=False, indent=2, default=str), encoding='utf-8')

def configure():
    sys.path.insert(0, str(VENDOR))
    import vgc.config as config
    config.SHOWDOWN_REPO = SHOWDOWN
    config.LOCAL_SERVER_HOST = '127.0.0.1:8765'
    config.LOCAL_SERVER_WS_URL = 'ws://127.0.0.1:8765/showdown/websocket'
    # The upstream worker has a separate macOS default captured in its signature.
    # Adapt paths only; keep vendor sources and strategic weights unchanged.
    import vgc.rl.env as env
    env.DEFAULT_SHOWDOWN_REPO = SHOWDOWN
    env.SimWorker.__init__.__defaults__ = (SHOWDOWN,)

def node_tool(*args):
    proc = subprocess.run([NODE, str(ROOT/'scripts/tooling.cjs'), *map(str,args)],
                          capture_output=True, text=True, encoding='utf-8', timeout=120)
    if proc.returncode:
        raise RuntimeError(proc.stderr.strip() or proc.stdout)
    return proc.stdout.strip()

def source_info():
    result = {}
    for name in ['pokemon-showdown', 'pokemon-vgc-ai', 'foul-play', 'vgc-bench']:
        repo = ROOT/'vendor'/name
        if (repo/'.git').exists():
            sha = subprocess.check_output(['git','-C',str(repo),'rev-parse','HEAD'], text=True).strip()
            dirty = subprocess.check_output(['git','-C',str(repo),'status','--porcelain','--untracked-files=no'],text=True).strip()
            result[name] = {'commit':sha, 'tracked_dirty':bool(dirty)}
    result['packages'] = {n:importlib.metadata.version(n) for n in ['poke-env','numpy','pokemon-vgc-ai']}
    return result

def battle_run(args):
    configure()
    import numpy as np
    from vgc.rl.env import SimWorker, DirectBattle
    from vgc.rl.agents import make_direct_agent

    if args.games < 1 or args.games % 2:
        raise ValueError('--games must be a positive even number (same-seed seat pairs)')
    teams = {'A':node_tool('team',args.team1,args.format), 'B':node_tool('team',args.team2,args.format)}
    policies = {'A':args.p1,'B':args.p2}
    out = ROOT/'runs'/datetime.now().strftime('%Y%m%d-%H%M%S-%f')
    out.mkdir(parents=True)
    metadata = {'format':args.format,'sources':source_info(),'policies':policies,
                'master_seed':args.seed,'python_hash_seed':0,'requested_games':args.games,'team_hashes':{k:hashlib.sha256(v.encode()).hexdigest() for k,v in teams.items()},
                'teams':teams,'information':'closed sheets, separate player streams; no private root exposed to policies',
                'assumed_spreads':'Allocation sources differ by team: see copied provenance sidecars. Screenshot baseline verified; psyspam SP assumed; Owen sand SP published.',
                'status':'running','created':datetime.now(timezone.utc).isoformat()}
    dump(out/'manifest.json',metadata)
    for actor, filename in [('A',args.team1),('B',args.team2)]:
        provenance=Path(filename).with_suffix('.provenance.json')
        if provenance.exists():
            dump(out/f'team-{actor}.provenance.json',json.loads(provenance.read_text(encoding='utf-8-sig')))
    outcomes=[]
    rng=random.Random(args.seed)
    os.environ['VGC_TRACE']='1'
    try:
        with SimWorker(SHOWDOWN) as worker:
            for pair in range(args.games//2):
                seed=[rng.randrange(1,65536) for _ in range(4)]
                for seat in range(2):
                    idx=pair*2+seat
                    assignment={'p1':'A' if seat==0 else 'B','p2':'B' if seat==0 else 'A'}
                    agents={s:make_direct_agent(policies[a],teams[a],battle_format=args.format) for s,a in assignment.items()}
                    battle=DirectBattle.start(worker,f'lab{idx}',teams[assignment['p1']],teams[assignment['p2']],battle_format=args.format,seed=seed)
                    logs={s:list(battle.last_lines[s]) for s in agents}
                    decisions=[]
                    calls={'A':0,'B':0}
                    started=time.monotonic()
                    try:
                        for s,agent in agents.items(): agent.observe(battle.battle_id,battle.last_lines[s])
                        while not battle.ended:
                            if len(decisions)>=args.max_decisions: raise RuntimeError('Decision limit; not recorded as a win/draw')
                            choices={}
                            for s in battle.sides_to_move():
                                actor=assignment[s]
                                calls[actor]+=1
                                policy_seed=int.from_bytes(hashlib.sha256(f'{args.seed}:{pair}:{actor}:{calls[actor]}'.encode()).digest()[:4],'big')
                                random.seed(policy_seed); np.random.seed(policy_seed)
                                before=time.monotonic()
                                choices[s]=agents[s].choose(battle.battles[s])
                                if getattr(agents[s].player,'fallback_count',0):
                                    raise RuntimeError(f'{policies[actor]} used random fallback; invalid measurement')
                                decisions.append({'side':s,'actor':actor,'turn':battle.battles[s].turn,
                                                  'choice':choices[s],'policy_seed':policy_seed,
                                                  'latency_ms':round((time.monotonic()-before)*1000,3)})
                            result=battle.step(choices)
                            for s,agent in agents.items():
                                logs[s].extend(result.lines[s]); agent.observe(battle.battle_id,result.lines[s])
                        for s,agent in agents.items(): agent.finish(battle.battles[s])
                        win=assignment.get(battle.winner)
                        outcome={'game':idx,'pair':pair,'seat':seat,'seed':seed,'assignment':assignment,'winner':win,
                                 'turns':battle.battles['p1'].turn,'seconds':round(time.monotonic()-started,3),'status':'complete'}
                        outcomes.append(outcome)
                        print(f'game {idx+1}/{args.games}: winner={win}, turns={outcome["turns"]}',flush=True)
                    finally:
                        for s,lines in logs.items():
                            (out/f'game-{idx:04d}.{s}.log').write_text('\n'.join(lines),encoding='utf-8')
                        dump(out/f'game-{idx:04d}.decisions.json',decisions)
                        dump(out/f'game-{idx:04d}.traces.json',{s:getattr(a.player,'decision_trace_history',[]) for s,a in agents.items()})
                        battle.close()
                    dump(out/'outcomes.json',outcomes)
        wins=sum(x['winner']=='A' for x in outcomes)
        pair_scores=[sum(1 if x['winner']=='A' else .5 if x['winner'] is None else 0 for x in outcomes if x['pair']==p)/2 for p in range(args.games//2)]
        # Resample independent seat-pairs, not individual correlated games.
        if len(pair_scores)>=2:
            bootstrap=np.random.default_rng(args.seed).choice(pair_scores,(3000,len(pair_scores)),replace=True).mean(axis=1)
            interval=np.quantile(bootstrap,[.025,.975]).tolist()
        else: interval=None
        summary={'games':len(outcomes),'A_wins':wins,'B_wins':sum(x['winner']=='B' for x in outcomes),
                 'draws':sum(x['winner'] is None for x in outcomes),'A_score':float(np.mean(pair_scores)),
                 'seat_pair_bootstrap_95':interval,'pair_scores':pair_scores,
                 'interpretation':'installation smoke / fixed-matchup diagnostic; not ladder strength or team-quality proof',
                 'sources':metadata['sources'],'artifacts':str(out)}
        dump(out/'summary.json',summary)
        metadata['status']='complete'
    except Exception as exc:
        metadata['status']='failed'; metadata['error']=repr(exc)
        raise
    finally:
        dump(out/'manifest.json',metadata)
        print(f'Artifacts: {out}',flush=True)
    return summary

def experiment_run(filename):
    spec=json.loads(Path(filename).read_text(encoding='utf-8-sig'))
    results={}
    for arm,team in spec['arms'].items():
        print(f'Experiment arm: {arm}',flush=True)
        results[arm]=battle_run(argparse.Namespace(
            p1=spec['policy'],p2=spec['opponent'],team1=ROOT/team,
            team2=ROOT/spec['opponent_team'],games=spec['games_per_arm'],
            seed=spec['seed'],format=spec['format'],max_decisions=600))
    labels=list(results)
    report={'specification':spec,'results':results}
    if len(labels)==2:
        import numpy as np
        delta=np.array(results[labels[1]]['pair_scores'])-np.array(results[labels[0]]['pair_scores'])
        report['comparison']={'direction':f'{labels[1]} minus {labels[0]}',
            'paired_score_delta':float(delta.mean()),'pair_differences':delta.tolist(),
            'bootstrap_95':np.quantile(np.random.default_rng(spec['seed']).choice(delta,(3000,len(delta)),replace=True).mean(axis=1),[.025,.975]).tolist() if len(delta)>1 else None,
            'caution':'Fixed matchup, assumed spreads, small diagnostic sample. No team-strength claim.'}
    path=ROOT/'runs'/('experiment-'+datetime.now().strftime('%Y%m%d-%H%M%S-%f')+'.json')
    dump(path,report)
    print(f'Experiment report: {path}')
    return report

def suite_run(filename):
    spec=json.loads(Path(filename).read_text(encoding='utf-8-sig'))
    report={'specification':spec,'matchups':{}}
    for name in spec['experiments']:
        report['matchups'][name]=experiment_run(ROOT/name)
    path=ROOT/'runs'/('suite-'+datetime.now().strftime('%Y%m%d-%H%M%S-%f')+'.json')
    dump(path,report)
    print(f'Suite report: {path}')

def main():
    p=argparse.ArgumentParser(description='Champions doubles AI laboratory')
    sub=p.add_subparsers(dest='command',required=True)
    sub.add_parser('doctor'); sub.add_parser('formats')
    v=sub.add_parser('validate'); v.add_argument('team',type=Path); v.add_argument('--format',default=FORMAT)
    c=sub.add_parser('calc'); c.add_argument('scenario',type=Path)
    e=sub.add_parser('experiment'); e.add_argument('specification',type=Path)
    s=sub.add_parser('suite'); s.add_argument('specification',type=Path)
    b=sub.add_parser('battle')
    b.add_argument('--p1',default='vgc_myopic',choices=['random','maxpower','heuristic','vgc_myopic','vgc','vgc_shallow','vgc_horizon'])
    b.add_argument('--p2',default='random',choices=['random','maxpower','heuristic','vgc_myopic','vgc','vgc_shallow','vgc_horizon'])
    b.add_argument('--team1',type=Path,default=ROOT/'teams/gravity-original.json')
    b.add_argument('--team2',type=Path,default=ROOT/'teams/psyspam-popular.json')
    b.add_argument('--games',type=int,default=2); b.add_argument('--seed',type=int,default=20260920)
    b.add_argument('--format',default=FORMAT); b.add_argument('--max-decisions',type=int,default=600)
    sub.add_parser('checks')
    sub.add_parser('server')
    args=p.parse_args()
    if args.command=='doctor':
        configure()
        print(json.dumps({'root':str(ROOT),'python':sys.version,'node':NODE,'format':FORMAT,'sources':source_info(),
                          'double_bots':'pokemon-vgc-ai + poke-env baselines installed',
                          'foul_play':'source only; singles; optional engine build not installed',
                          'vgc_bench':'reference source only; separate training dependencies required'},indent=2))
    elif args.command=='formats': print(node_tool('formats'))
    elif args.command=='validate': print(node_tool('team',args.team,args.format))
    elif args.command=='calc': print(node_tool('calc',args.scenario))
    elif args.command=='battle': print(json.dumps(battle_run(args),indent=2))
    elif args.command=='experiment': experiment_run(args.specification)
    elif args.command=='suite': suite_run(args.specification)
    elif args.command=='server':
        config=(SHOWDOWN/'config/config.js').read_text(encoding='utf-8')
        if "exports.bindaddress = '127.0.0.1'" not in config:
            raise RuntimeError('Server requires config.bindaddress = 127.0.0.1; run setup.ps1 first')
        subprocess.run([NODE,'pokemon-showdown','start','8765','--no-security'],cwd=SHOWDOWN,check=True)
    elif args.command=='checks':
        configure()
        import pytest
        sys.path.insert(0,str(VENDOR/'tests'))
        paths=['tests/test_stats_ground_truth.py','tests/test_damage_ground_truth.py']
        subprocess.run([NODE,str(ROOT/'scripts/calibration.cjs')],check=True)
        print('Upstream stale M-B literal assertion is replaced by our real M-C format/bring-four probe.')
        raise SystemExit(pytest.main(['-q','-m','integration','-k','not test_format_is_doubles_and_uses_champions_mod',
                                     '--junitxml='+str(ROOT/'runs/ground-truth.xml'),*[str(VENDOR/x) for x in paths]]))

if __name__=='__main__':
    main()



