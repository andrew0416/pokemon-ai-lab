"""New seeds and matrix families, independent strategic-form BR, optional required LP.

Fixture builders and the oracle are extracted from the inherited Python oracle,
not from the Rust solver. Local checks do not need SciPy; CI requires it.
Different rules need not return identical policies. Every certificate must match
exhaustive pure plans for the actual exported game, including partial growth.
"""
from pathlib import Path
import argparse,ast,copy,hashlib,itertools,json,math,random,subprocess
import numpy as np

HERE=Path(__file__).resolve().parent
def helpers(path,names):
    module=ast.parse(path.read_text(encoding='utf-8'))
    selected=[n for n in module.body if isinstance(n,ast.FunctionDef) and n.name in names]
    assert {n.name for n in selected}==set(names)
    scope=dict(np=np,itertools=itertools)
    exec(compile(ast.Module(body=selected,type_ignores=[]),str(path),'exec'),scope)
    return [scope[n] for n in names]
generated,kuhn,normal_form=helpers(HERE.parent/'s26_tree/check.py',['generated','kuhn','normal_form'])
observed,canonical=helpers(HERE.parent/'s26_tree/growing_check.py',['generated','canonical'])
RULES=['lcfr','cfr','cfr-simultaneous','dcfr','pcfr+','sapcfr+','hs-dcfr-15','hs-dcfr-30','hs-pcfr-15','hs-pcfr-30']

def matrix_case(rng,i):
    # Chance after each joint choice, varying player order and utility offsets.
    rows=2+i%3;cols=2+(i//3)%2;nodes=[None];children=[]
    def add(n):nodes.append(n);return len(nodes)-1
    first=i%2;other=1-first
    for a in range(rows):
        n=add(None);children.append(n);leaves=[]
        for b in range(cols):
            value=rng.randrange(-10,11)+(1000 if i%4==0 else 0)
            chance=add(None);leaves.append(chance)
            x=add(dict(type='terminal',value=value));y=add(dict(type='terminal',value=-value/3))
            p=0. if i%5==0 else (i%3+1)/4
            nodes[chance]=dict(type='chance',edges=[dict(probability=p,child=x),dict(probability=1-p,child=y)])
        nodes[n]=dict(type='decision',player=other,information='second',actions=[f'c{b}' for b in range(cols)],children=leaves)
    nodes[0]=dict(type='decision',player=first,information='first',actions=[f'r{a}' for a in range(rows)],children=children)
    return dict(mode='tree',root=0,nodes=nodes,include_keys=True,solver=dict(iterations=4096,tolerance=.02,check_every=32))

def main():
    p=argparse.ArgumentParser();p.add_argument('--binary',type=Path,required=True);p.add_argument('--out',type=Path,required=True)
    p.add_argument('--require-lp',action='store_true');p.add_argument('--small',action='store_true');a=p.parse_args()
    linprog=None
    if a.require_lp:from scipy.optimize import linprog
    a.out.mkdir(parents=True,exist_ok=False);binary=a.binary.resolve();rng=random.Random(261003229);records=[];lp_cache={}
    def run(name,q,ok=True):
        path=a.out/(name+'-request.json');path.write_text(json.dumps(q),encoding='utf-8')
        child=subprocess.run([str(binary),str(path.resolve())],capture_output=True,timeout=90)
        (a.out/(name+'-result.json')).write_bytes(child.stdout);(a.out/(name+'-stderr.txt')).write_bytes(child.stderr)
        assert (child.returncode==0)==ok,(name,child.stderr.decode(errors='replace'))
        return json.loads(child.stdout) if ok else None
    def certify(name,q,r):
        game=r.get('oracle_tree',q);key=json.dumps(dict(root=game['root'],nodes=game['nodes']),sort_keys=True)
        if key not in lp_cache:
            keys,plans,A=normal_form(game);n,m=A.shape;value=None
            if linprog:
                lp=linprog(np.r_[np.zeros(n),-1.],A_ub=np.c_[-A.T,np.ones(m)],b_ub=np.zeros(m),
                    A_eq=np.array([np.r_[np.ones(n),0.]]),b_eq=[1.],bounds=[(0,None)]*n+[(None,None)],method='highs')
                assert lp.success,lp.message
                value=-lp.fun
            lp_cache[key]=keys,plans,A,value
        keys,plans,A,value=lp_cache[key];lookup={i['key']:i for i in r['policies']}
        assert set(lookup)==set(keys[0]+keys[1])
        for v in lookup.values():assert min(v['probabilities'])>=0 and abs(sum(v['probabilities'])-1)<1e-10
        pure=[np.array([math.prod(lookup[k]['probabilities'][act] for k,act in zip(keys[player],plan)) for plan in plans[player]]) for player in range(2)]
        lower=float(min(pure[0]@A));upper=float(max(A@pure[1]));profile=float(pure[0]@A@pure[1]);gap=upper-lower
        for k,v in [('value',profile),('lower',lower),('upper',upper),('finite_game_gap',gap)]:
            assert abs(r[k]-v)<1e-7,(name,k,r[k],v)
        if value is not None:assert lower-1e-7<=value<=upper+1e-7,(name,value,lower,upper)
        converge=r.get('surrogate_converged',r['converged'])
        assert converge==(r['finite_game_gap']<=q['solver']['tolerance'])
        paper=r.get('paper',(r.get('metadata',{}).get('growth') or {}).get('paper',{})) or {}
        records.append(dict(case=name,gap=gap,iterations=r['iterations'],converged=converge,lp_value=value,paper=paper))
        return value
    tree_count=4 if a.small else 16;matrix_count=4 if a.small else 12
    cases=[(f'new-hidden-{i}',generated(rng,i)) for i in range(tree_count)]
    cases += [(f'matrix-{i}',matrix_case(rng,i)) for i in range(matrix_count)]+[('kuhn',kuhn())]
    for name,q in cases:
        q['solver'].update(iterations=8192,tolerance=.02)
        original=run(name+'-original',q);certify(name+'-original',q,original)
        variants=[(dict(rule=r,sequence=True,checks='periodic'),dict(compressed_checks=True)) for r in ['lcfr','cfr-simultaneous','dcfr','sapcfr+']]
        variants.append((dict(sequence=True),{}))
        reference_request=copy.deepcopy(q);reference_request['solver']['paper']=dict(sequence=True)
        reference=run(name+'-sequence',reference_request);certify(name+'-sequence',reference_request,reference)
        for i,(setting,pipeline) in enumerate(variants):
            request=copy.deepcopy(q);request['solver'].update(paper=setting,pipeline=pipeline);tag=name+f'-p{i}'
            r=run(tag,request);value=certify(tag,request,r)
            if not pipeline:
                for field in ['policies','value','lower','upper','finite_game_gap','iterations','converged']:assert r[field]==reference[field],(name,field)
            if name=='kuhn' and value is not None:assert abs(value+1/18)<1e-10
    allflags=dict(frontier_index=True,owned_compiler=True,incremental_sequence=True,compressed_checks=True)
    growth_settings=[(dict(sequence=True),p) for p in [dict(),dict(frontier_index=True),dict(owned_compiler=True),dict(owned_compiler=True,incremental_sequence=True),dict(compressed_checks=True),allflags]]
    growth_settings += [(dict(sequence=True,reuse_policy=True),allflags),(dict(sequence=True,rule='cfr-simultaneous',warm_iterations=8),allflags)]
    growth_count=3 if a.small else 16
    for i in range(growth_count):
        q=observed(rng,i);q['solver'].update(iterations=8192,tolerance=.03,paper=dict(sequence=True))
        full=run(f'growth-{i}-full',q);certify(f'growth-{i}-full',q,full)
        for budget in [1,2,99]:
            control=copy.deepcopy(q);control['growth']=dict(cadence=4,max_expansions=budget,max_walks=1000,seed=i+1)
            original=run(f'growth-{i}-budget{budget}-control',control);certify(f'growth-{i}-budget{budget}-control',control,original)
            for v,(setting,pipeline) in enumerate(growth_settings):
                request=copy.deepcopy(control);request['solver'].update(paper=setting,pipeline=pipeline)
                tag=f'growth-{i}-v{v}-budget{budget}';r=run(tag,request);certify(tag,request,r)
                g=r['metadata']['growth'];assert g['committed_expansions']==(3 if budget==99 else budget)
                assert r['converged']==(g['horizon_complete'] and r['surrogate_converged'])
                if v<3:
                    for field in ['policies','value','lower','upper','finite_game_gap','iterations','converged']:assert r[field]==original[field],(tag,field)
                    for field in ['walks','solves','total_cfr_iterations','attempted_transitions','committed_expansions']:assert g[field]==original['metadata']['growth'][field],(tag,field)
                if budget==99:assert canonical(r['oracle_tree'])==canonical(full['oracle_tree'])
                else:assert r['full_horizon_gap'] is None and r['certificate_scope']=='current-fixed-leaf-surrogate-only'
    # Abort after a complete pending group plus part of the next: no unsolved publish.
    q=observed(rng,2);q['solver'].update(paper=dict(sequence=True),pipeline=allflags);q['growth']=dict(max_expansions=1,seed=1,cadence=4)
    root=run('rollback-root',q);q['growth'].update(max_expansions=2,cadence=1);one=run('rollback-one',q)
    cap=one['metadata']['growth']['attempted_transitions']+1;q['growth'].update(max_expansions=100,cadence=4);q['limits']['max_transitions']=cap
    r=run('rollback-batch',q);certify('rollback-batch',q,r)
    assert r['policies']==root['policies'] and canonical(r['oracle_tree'])==canonical(root['oracle_tree'])
    g=r['metadata']['growth'];assert g['attempted_transitions']==cap and g['committed_expansions']==1 and g['solves']==1 and g['stop']=='TransitionLimit'
    for index,pipeline in enumerate([dict(unknown=True),dict(frontier_index=1),dict(incremental_sequence=True)]):
        q['solver'].update(paper=dict(sequence=True),pipeline=pipeline);run(f'invalid-pipeline-{index}',q,False)
    q['solver'].update(paper=dict(),pipeline=dict(compressed_checks=True));run('invalid-check-kernel',q,False)
    q['solver'].update(paper=dict(sequence=True,compact=True),pipeline=allflags);run('invalid-compact',q,False)
    q['solver'].pop('paper');run('missing-explicit-paper',q,False)
    fixed=kuhn();fixed['solver'].update(paper=dict(sequence=True),pipeline=dict(owned_compiler=True));run('invalid-fixed-growth-flag',fixed,False)
    summary=dict(passed=True,certificates=len(records),distinct_fixed_games=len(cases),distinct_growing_games=growth_count,
        new_seed=261003229,independent_exhaustive_best_responses=True,independent_lp=a.require_lp,
        converged=sum(r['converged'] for r in records),warm_applications=sum(r['paper'].get('warm_applied',0) for r in records),
        reuse_acceptances=sum(r['paper'].get('reused',0) for r in records),atomic_rollback=True,performance=False,
        binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),cases=records)
    (a.out/'summary.json').write_text(json.dumps(summary,indent=2)+'\n',encoding='utf-8');print(json.dumps({k:v for k,v in summary.items() if k!='cases'}))
if __name__=='__main__':main()
