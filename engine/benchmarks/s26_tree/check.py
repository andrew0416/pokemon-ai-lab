"""Independent strategic-form LP and exact best responses for finite information trees."""
from pathlib import Path
import argparse,hashlib,itertools,json,math,random,subprocess
import numpy as np
from scipy.optimize import linprog

def generated(rng,case):
 nodes=[None];worlds=1+case%3
 weights=[float(1+rng.randrange(9)) for _ in range(worlds)]
 if worlds>1 and case%7==0:weights[0]=0.
 weights=[v/sum(weights) for v in weights]
 def add(value):nodes.append(value);return len(nodes)-1
 edges=[]
 for w in range(worlds):
  root=add(None);edges.append(dict(probability=weights[w],child=root));rows=[]
  for a in range(2):
   col=add(None);rows.append(col);columns=[]
   for c in range(2):
    chance=add(None);columns.append(chance);signals=[]
    signal_p=[0.2,0.5,0.8][(case+w+c)%3]
    for signal in range(2):
     us=add(None);signals.append(dict(probability=signal_p if signal==0 else 1-signal_p,child=us));leaves=[]
     for b in range(2):leaves.append(add(dict(type='terminal',value=rng.randrange(-5,6))))
     # Our first action is remembered; neither world nor hidden opponent action is known.
     nodes[us]=dict(type='decision',player=0,information=f'us/a{a}/signal{signal}',actions=['guess0','guess1'],children=leaves)
    nodes[chance]=dict(type='chance',edges=signals)
   nodes[col]=dict(type='decision',player=1,information=f'them/type{w}',actions=['commit0','commit1'],children=columns)
  nodes[root]=dict(type='decision',player=0,information='us/root',actions=['start0','start1'],children=rows)
 nodes[0]=dict(type='chance',edges=edges)
 return dict(mode='tree',root=0,nodes=nodes,include_keys=True,solver=dict(iterations=50000,tolerance=0.01,check_every=32))

def kuhn():
 nodes=[None];edges=[]
 def visit(cards,h):
  sign=1 if cards[0]>cards[1] else -1
  terminal={'cc':sign,'bf':1,'bc':2*sign,'cbf':-1,'cbc':2*sign}
  i=len(nodes);nodes.append(None)
  if h in terminal:nodes[i]=dict(type='terminal',value=terminal[h]);return i
  player=0 if h in ('','cb') else 1;actions=['f','c'] if h.endswith('b') else ['c','b']
  nodes[i]=dict(type='decision',player=player,information=f'p{player}/card{cards[player]}/{h}',actions=actions,children=[visit(cards,h+a) for a in actions]);return i
 for a,b in itertools.permutations(range(3),2):edges.append(dict(probability=1/6,child=visit([a,b],'')))
 nodes[0]=dict(type='chance',edges=edges)
 return dict(mode='tree',root=0,nodes=nodes,include_keys=True,solver=dict(iterations=50000,tolerance=.003,check_every=32))

def normal_form(request):
 info=[{},{}]
 for n in request['nodes']:
  if n['type']=='decision':info[n['player']][n['information']]=len(n['actions'])
 keys=[list(x) for x in info];plans=[list(itertools.product(*[range(m) for m in x.values()])) for x in info]
 def value(node,chosen):
  n=request['nodes'][node]
  if n['type']=='terminal':return n['value']
  if n['type']=='chance':return sum(e['probability']*value(e['child'],chosen) for e in n['edges'])
  return value(n['children'][chosen[n['player']][n['information']]],chosen)
 A=np.array([[value(request['root'],[dict(zip(keys[0],p)),dict(zip(keys[1],q))]) for q in plans[1]] for p in plans[0]])
 return keys,plans,A

def main():
 p=argparse.ArgumentParser();p.add_argument('--binary',type=Path,required=True);p.add_argument('--out',type=Path,required=True);a=p.parse_args()
 a.out.mkdir(parents=True,exist_ok=False);binary=a.binary.resolve();rng=random.Random(26100226);records=[]
 cases=[(f'generated-{i:02}',generated(rng,i)) for i in range(48)]+[('kuhn',kuhn())]
 for name,request in cases:
  path=a.out/(name+'-request.json');path.write_text(json.dumps(request),encoding='utf-8')
  run=subprocess.run([str(binary),str(path)],capture_output=True,text=True,timeout=45)
  (a.out/(name+'-stderr.txt')).write_text(run.stderr,encoding='utf-8');assert run.returncode==0,run.stderr
  (a.out/(name+'-result.json')).write_text(run.stdout,encoding='utf-8');r=json.loads(run.stdout)
  keys,plans,A=normal_form(request);n,m=A.shape
  lp=linprog(np.r_[np.zeros(n),-1.],A_ub=np.c_[-A.T,np.ones(m)],b_ub=np.zeros(m),A_eq=np.array([np.r_[np.ones(n),0.]]),b_eq=[1.],bounds=[(0,None)]*n+[(None,None)],method='highs')
  assert lp.success,lp.message
  value=-lp.fun;lookup={v['key']:v for v in r['policies']}
  assert set(lookup)==set(keys[0]+keys[1])
  for v in lookup.values():assert min(v['probabilities'])>=0 and abs(sum(v['probabilities'])-1)<1e-10
  pure=[np.array([math.prod(lookup[k]['probabilities'][action] for k,action in zip(keys[player],plan)) for plan in plans[player]]) for player in range(2)]
  assert all(abs(sum(v)-1)<1e-9 for v in pure)
  lower=float(min(pure[0]@A));upper=float(max(A@pure[1]));profile=float(pure[0]@A@pure[1]);gap=upper-lower
  for key,expected in [('value',profile),('lower',lower),('upper',upper),('finite_game_gap',gap)]:assert abs(r[key]-expected)<1e-8,(name,key,r[key],expected)
  assert lower-1e-7<=value<=upper+1e-7,(name,value,lower,upper)
  assert r['converged']==(r['finite_game_gap']<=request['solver']['tolerance'])
  assert gap<=.025,(name,gap)
  if name=='kuhn':assert abs(value+1/18)<1e-10
  records.append(dict(case=name,nodes=len(request['nodes']),row_plans=n,column_plans=m,lp_value=value,lower=lower,upper=upper,gap=gap,iterations=r['iterations'],converged=r['converged']))
 summary=dict(schema=1,passed=len(records),converged=sum(r['converged'] for r in records),max_gap=max(r['gap'] for r in records),binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),performance=False,cases=records)
 (a.out/'summary.json').write_text(json.dumps(summary,indent=2)+'\n',encoding='utf-8');print(json.dumps({k:v for k,v in summary.items() if k!='cases'}))
if __name__=='__main__':main()
