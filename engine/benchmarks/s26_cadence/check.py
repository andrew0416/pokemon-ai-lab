"""Independent normal-form LP for cadence snapshots, with pending-batch rollback.

24 existing generated games; 168 certificates across full and cadence2/4 budgets.
This is not 168 distinct new games, nor a win-rate test.
"""
from pathlib import Path
import argparse,copy,hashlib,json,random,subprocess,sys
sys.path.insert(0,str(Path(__file__).resolve().parent.parent/'s26_tree'))
from growing_check import generated,canonical,certificate

def main():
    p=argparse.ArgumentParser();p.add_argument('--binary',type=Path,required=True);p.add_argument('--out',type=Path,required=True);a=p.parse_args()
    a.out.mkdir(parents=True,exist_ok=False);binary=a.binary.resolve();rng=random.Random(26100227);records=[]
    def run(name,request,ok=True):
        path=a.out/(name+'-request.json');path.write_text(json.dumps(request),encoding='utf-8')
        child=subprocess.run([str(binary),str(path.resolve())],capture_output=True,timeout=90)
        (a.out/(name+'-result.json')).write_bytes(child.stdout);(a.out/(name+'-stderr.txt')).write_bytes(child.stderr)
        assert (child.returncode==0)==ok,(name,child.stderr.decode())
        return json.loads(child.stdout) if ok else None
    for case in range(24):
        request=generated(rng,case);name=f'case-{case:02}'
        full=run(name+'-full',request);ref=certificate(full);records.append(dict(case=name+'-full',**ref))
        for cadence in [2,4]:
            for budget in [1,2,99]:
                q=copy.deepcopy(request);q['growth']=dict(cadence=cadence,max_expansions=budget,max_walks=1000,seed=case+1)
                tag=name+f'-cadence{cadence}-budget{budget}';r=run(tag,q);cert=certificate(r);g=r['metadata']['growth']
                assert g['solve_cadence']==cadence
                assert r['surrogate_converged']==(r['finite_game_gap']<=q['solver']['tolerance'])
                assert r['converged']==(g['horizon_complete'] and r['surrogate_converged'])
                assert g['attempted_transitions']==g['committed_transitions']
                expected=3 if budget==99 else budget
                assert g['committed_expansions']==expected and g['frontier_public_groups']==3-expected
                if budget==99:
                    assert g['horizon_complete'] and r['converged'] and g['solves']==2
                    assert canonical(r['oracle_tree'])==canonical(full['oracle_tree'])
                    assert {i['key']:i['probabilities'] for i in r['policies']}=={i['key']:i['probabilities'] for i in full['policies']}
                    assert abs(cert['lp_value']-ref['lp_value'])<1e-8
                    assert g['attempted_transitions']==full['metadata']['transitions']
                else:
                    assert not g['horizon_complete'] and r['full_horizon_gap'] is None
                    assert r['certificate_scope']=='current-fixed-leaf-surrogate-only'
                records.append(dict(case=tag,**cert))
    q=generated(rng,2);q['growth']=dict(max_expansions=1,seed=1,cadence=4)
    root=run('rollback-root',q)
    q['growth'].update(max_expansions=2,cadence=1);one=run('rollback-one-group',q)
    cap=one['metadata']['growth']['attempted_transitions']+1
    q['growth'].update(max_expansions=100,cadence=4);q['limits']['max_transitions']=cap
    r=run('rollback-pending-complete-group-plus-partial',q)
    assert r['policies']==root['policies'] and canonical(r['oracle_tree'])==canonical(root['oracle_tree'])
    g=r['metadata']['growth'];assert g['attempted_transitions']==cap and g['committed_expansions']==1 and g['solves']==1
    assert g['stop']=='TransitionLimit' and not g['horizon_complete'] and r['full_horizon_gap'] is None
    certificate(r)
    q['limits']['max_transitions']=root['metadata']['growth']['attempted_transitions']-1;run('incomplete-root',q,False)
    q['growth']['cadence']=3;run('invalid-cadence',q,False)
    summary=dict(passed=len(records),generated_games=24,lp_certificates=len(records),cadences=[2,4],full_growth_matches=48,
        pending_batch_rollback=True,root_rejection=True,invalid_cadence=True,performance=False,
        cases=records,binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest())
    (a.out/'summary.json').write_text(json.dumps(summary,indent=2)+'\n',encoding='utf-8')
    print(json.dumps({k:v for k,v in summary.items() if k!='cases'}))
if __name__=='__main__':main()
