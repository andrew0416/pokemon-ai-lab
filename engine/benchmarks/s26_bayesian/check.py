"""Independent LP/certificate checks through the real CLI; remote correctness, not speed."""
from pathlib import Path
import argparse, hashlib, json, math, random, subprocess
import numpy as np
from scipy.optimize import linprog

p=argparse.ArgumentParser();p.add_argument('--binary',type=Path,required=True);p.add_argument('--out',type=Path,required=True);a=p.parse_args()
a.out.mkdir(parents=True,exist_ok=False);binary=a.binary.resolve()
rng=random.Random(2601002);records=[]
for case in range(72):
 n=2+case%4;k=1+case%6
 worlds=[]
 for w in range(k):
  m=1+(case+2*w)%5
  weights=0 if k>1 and w==0 and case%7==0 else 1+rng.randrange(10)
  worlds.append(dict(id=f'w{w}',weight=weights,columns=[f'c{c}' for c in range(m)],payoffs=[rng.randrange(-5,6) for _ in range(n*m)]))
 request=dict(mode='matrix',rows=[f'r{r}' for r in range(n)],worlds=worlds,solver=dict(iterations=100000,tolerance=0.02,check_every=64))
 if case%3==0:
  request['likelihoods']=[dict(world=w['id'],probability=(i+1)/(k+1)) for i,w in reversed(list(enumerate(worlds)))]
 path=a.out/f'{case:02d}-request.json';path.write_text(json.dumps(request),encoding='utf-8')
 child=subprocess.run([str(binary),str(path)],capture_output=True,text=True,timeout=30)
 (a.out/f'{case:02d}-stderr.txt').write_text(child.stderr,encoding='utf-8')
 assert child.returncode==0,child.stderr
 (a.out/f'{case:02d}-result.json').write_text(child.stdout,encoding='utf-8')
 result=json.loads(child.stdout)
 policy=np.array([v['probability'] for v in result['ours']]);assert abs(sum(policy)-1)<1e-10 and min(policy)>=0
 prior=np.array([w['weight'] for w in worlds],dtype=float)
 if 'likelihoods' in request:
  lookup={v['world']:v['probability'] for v in request['likelihoods']};prior*=np.array([lookup[w['id']] for w in worlds])
 prior/=sum(prior)
 assert np.max(np.abs(prior-np.array([w['posterior'] for w in result['worlds']])))<1e-12
 matrices=[np.array(w['payoffs'],dtype=float).reshape(n,len(w['columns'])) for w in worlds]
 # LP maximizes sum_w b_w*v_w subject to v_w <= p dot A_w[:,c].
 objective=np.concatenate([np.zeros(n),-prior]);constraints=[]
 for i,A in enumerate(matrices):
  for c in range(A.shape[1]):
   row=np.zeros(n+k);row[:n]=-A[:,c];row[n+i]=1;constraints.append(row)
 equality=np.concatenate([np.ones(n),np.zeros(k)])[None,:]
 lp=linprog(objective,A_ub=np.array(constraints),b_ub=np.zeros(len(constraints)),A_eq=equality,b_eq=[1.],bounds=[(0,None)]*n+[(None,None)]*k,method='highs')
 assert lp.success,lp.message
 reference=-lp.fun
 lower=0.;row_values=np.zeros(n);profile=0.
 for i,(w,A) in enumerate(zip(result['worlds'],matrices)):
  assert w['id']==worlds[i]['id'] and [v['action'] for v in w['theirs']]==worlds[i]['columns']
  q=np.array([v['probability'] for v in w['theirs']]);assert abs(sum(q)-1)<1e-10 and min(q)>=0
  lower+=prior[i]*min(policy@A);row_values+=prior[i]*(A@q)
  world_value=float(policy@A@q);profile+=prior[i]*world_value
  assert abs(world_value-w['value'])<1e-10
 upper=max(row_values);gap=upper-lower
 for key,expected in [('value',profile),('lower',lower),('upper',upper),('fixed_matrix_gap',gap)]:assert abs(result[key]-expected)<1e-9,(case,key)
 assert lower-1e-7<=reference<=upper+1e-7,(case,reference,lower,upper)
 assert lower-1e-9<=profile<=upper+1e-9
 assert result['converged']==(result['fixed_matrix_gap']<=request['solver']['tolerance'])
 assert gap<=0.05,(case,gap)
 records.append(dict(case=case,rows=n,worlds=k,columns=[len(w['columns']) for w in worlds],lp_value=reference,gap=gap,converged=result['converged'],iterations=result['iterations']))
summary=dict(schema=1,binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),cases=records,passed=len(records),max_gap=max(v['gap'] for v in records),converged=sum(v['converged'] for v in records),performance=False)
(a.out/'summary.json').write_text(json.dumps(summary,indent=2)+'\n',encoding='utf-8')
print(json.dumps({k:v for k,v in summary.items() if k!='cases'}))
