"""Independent LP certificates for growing snapshots; exhaustive-reference differential.

No wall-time/speed claims. Every generated fixture has two public continuation groups,
hidden worlds/commitments, and perfect-recall own-action histories.
"""
from pathlib import Path
import argparse,copy,hashlib,json,math,random,subprocess
import numpy as np
from scipy.optimize import linprog
from check import normal_form

def generated(rng,case):
    states=[]; worlds=[]; width=1+case%3
    def add(v):states.append(v);return len(states)-1
    for w in range(width):
        root=add(None);worlds.append(dict(id=f'w{w}',weight=w+1,position=root));rows=[]
        for a in range(2):
            cols=[]
            for c in range(2):
                edges=[];p=[.2,.5,.8][(case+w+c)%3]
                for signal in range(2):
                    second=add(None);edges.append(dict(probability=p if signal==0 else 1-p,to=second));leaves=[]
                    for b in range(2):
                        end=add(dict(phase='terminal',value=rng.randrange(-6,7),public=f'end/{signal}',private=['','']))
                        leaves.append([[dict(probability=1.,to=end)]])
                    states[second]=dict(phase='turn',value=rng.randrange(-2,3),public=f'signal/{signal}',
                        private=['',''],actions=[['guess0','guess1'],['wait']],transitions=leaves)
                cols.append(edges)
            rows.append(cols)
        states[root]=dict(phase='turn',value=0,public='root',private=['',''],
            actions=[['start0','start1'],['commit0','commit1']],transitions=rows)
    return dict(mode='observed',states=states,worlds=worlds,include_keys=True,limits=dict(turns=2),
        solver=dict(iterations=20000,tolerance=.02,check_every=32))

def canonical(game):
    def walk(n):
        v=game['nodes'][n]
        if v['type']=='terminal':return ('terminal',v['value'])
        if v['type']=='chance':return ('chance',tuple((e['probability'],walk(e['child'])) for e in v['edges']))
        return ('decision',v['player'],v['information'],tuple(v['actions']),tuple(walk(c) for c in v['children']))
    return walk(game['root'])

def certificate(r):
    keys,plans,A=normal_form(r['oracle_tree']);n,m=A.shape
    lp=linprog(np.r_[np.zeros(n),-1.],A_ub=np.c_[-A.T,np.ones(m)],b_ub=np.zeros(m),
        A_eq=np.array([np.r_[np.ones(n),0.]]),b_eq=[1.],bounds=[(0,None)]*n+[(None,None)],method='highs')
    assert lp.success,lp.message
    lookup={i['key']:i for i in r['policies']}
    assert set(lookup)==set(keys[0]+keys[1])
    pure=[np.array([math.prod(lookup[k]['probabilities'][a] for k,a in zip(keys[player],plan)) for plan in plans[player]]) for player in range(2)]
    assert all(abs(v.sum()-1)<1e-8 for v in pure)
    lower=float(min(pure[0]@A));upper=float(max(A@pure[1]));value=float(pure[0]@A@pure[1]);gap=upper-lower
    for k,expected in [('lower',lower),('upper',upper),('value',value),('finite_game_gap',gap)]:
        assert abs(r[k]-expected)<1e-7,(k,r[k],expected)
    assert lower-1e-7<=-lp.fun<=upper+1e-7
    return dict(lp_value=-lp.fun,lower=lower,upper=upper,gap=gap)

def main():
    p=argparse.ArgumentParser();p.add_argument('--binary',type=Path,required=True);p.add_argument('--out',type=Path,required=True);a=p.parse_args()
    a.out.mkdir(parents=True,exist_ok=False);binary=a.binary.resolve();rng=random.Random(26100227);records=[]
    def run(name,request,ok=True):
        path=a.out/(name+'-request.json');path.write_text(json.dumps(request),encoding='utf-8')
        child=subprocess.run([str(binary),str(path)],capture_output=True,timeout=90)
        (a.out/(name+'-result.json')).write_bytes(child.stdout);(a.out/(name+'-stderr.txt')).write_bytes(child.stderr)
        assert (child.returncode==0)==ok,(name,child.stderr.decode())
        return json.loads(child.stdout) if ok else None
    for case in range(24):
        request=generated(rng,case);name=f'case-{case:02}'
        full=run(name+'-full',request);ref=certificate(full)
        assert full['converged']
        records.append(dict(case=name+'-full',**ref))
        for budget in (1,2,99):
            q=copy.deepcopy(request);q['growth']=dict(max_expansions=budget,max_walks=1000,seed=case+1)
            r=run(name+f'-budget{budget}',q);cert=certificate(r);g=r['metadata']['growth']
            assert r['surrogate_converged']==(r['finite_game_gap']<=q['solver']['tolerance'])
            assert r['converged']==(g['horizon_complete'] and r['surrogate_converged'])
            assert g['attempted_transitions']==g['committed_transitions']
            expected=3 if budget==99 else budget
            assert g['committed_expansions']==expected
            assert g['frontier_public_groups']==3-expected
            # One shared root and two own-action memories for each admitted signal.
            ours=[i for i in r['policies'] if i['player']==0]
            assert len(ours)==1+2*(expected-1)
            if budget==99:
                assert g['horizon_complete'] and r['converged']
                assert canonical(r['oracle_tree'])==canonical(full['oracle_tree'])
                assert abs(cert['lp_value']-ref['lp_value'])<1e-8
                assert g['attempted_transitions']==full['metadata']['transitions']
            else:
                assert not g['horizon_complete'] and r['full_horizon_gap'] is None
                assert r['certificate_scope']=='current-fixed-leaf-surrogate-only'
            records.append(dict(case=name+f'-budget{budget}',horizon_complete=g['horizon_complete'],**cert))
    # Reject half an admission, retain the exact previous policy and account spent work.
    q=generated(rng,2);q['growth']=dict(max_expansions=1,seed=1)
    base=run('rollback-base',q);calls=base['metadata']['growth']['attempted_transitions']
    q['growth']['max_expansions']=100;q['limits']['max_transitions']=calls+1
    r=run('rollback-transition',q);assert r['policies']==base['policies']
    assert canonical(r['oracle_tree'])==canonical(base['oracle_tree'])
    assert r['metadata']['growth']['stop']=='TransitionLimit'
    assert r['metadata']['growth']['attempted_transitions']==calls+1
    q['limits']['max_transitions']=calls-1;run('incomplete-root',q,False)
    summary=dict(passed=len(records),generated_games=24,lp_certificates=len(records),
        full_growth_matches=24,root_rejection=True,atomic_rollback=True,performance=False,
        max_gap=max(x['gap'] for x in records),cases=records,binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest())
    (a.out/'summary.json').write_text(json.dumps(summary,indent=2)+'\n',encoding='utf-8')
    print(json.dumps({k:v for k,v in summary.items() if k!='cases'}))
if __name__=='__main__':main()
