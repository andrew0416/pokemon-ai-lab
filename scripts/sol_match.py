import sys,json,threading,hashlib
from pathlib import Path
from datetime import datetime
from http.server import HTTPServer,BaseHTTPRequestHandler
sys.path.insert(0,'D:/pokemon-ai-lab/scripts')
import lab
lab.configure()
from vgc.rl.env import SimWorker,DirectBattle
out=lab.ROOT/'runs'/('sol-match-'+datetime.now().strftime('%Y%m%d-%H%M%S'));out.mkdir()
worker=SimWorker(lab.SHOWDOWN)
files={'p1':lab.ROOT/'teams/taunt-panel/own-starmie.json','p2':lab.ROOT/'teams/library/doubles/m-c/sand-owen/team.json'}
teams={s:lab.node_tool('team',p,lab.FORMAT) for s,p in files.items()}
b=DirectBattle.start(worker,'solmatch',teams['p1'],teams['p2'],battle_format=lab.FORMAT,seed=[9182,7431,5227,201])
logs={s:list(b.last_lines[s]) for s in ['p1','p2']};decisions=[]
b.step({'p1':'team 1235','p2':'team 5612'})
for s in logs:logs[s]+=b.last_lines[s]
manifest={'format':lab.FORMAT,'seed':[9182,7431,5227,201],'models':'two gpt-5.6-sol agents','forced_preview':{'p1':'1235 Sableye Milotic Starmie Flapple','p2':'5612 Tyranitar Excadrill Salamence Indeedee'},'team_hashes':{s:hashlib.sha256(p.read_bytes()).hexdigest() for s,p in files.items()},'scope':'One pilot game, not a win-rate estimate','open_sets':True}
lab.dump(out/'manifest.json',manifest)
def save():
 for s in logs:(out/f'{s}.log').write_text('\n'.join(logs[s]),encoding='utf8')
 lab.dump(out/'decisions.json',decisions)
 if b.ended:lab.dump(out/'result.json',{'winner':b.winner,'turn':b.battles['p1'].turn})
save()
class H(BaseHTTPRequestHandler):
 def log_message(self,*a):pass
 def reply(self,x):
  data=json.dumps(x,ensure_ascii=False).encode();self.send_response(200);self.send_header('Content-Type','application/json; charset=utf-8');self.end_headers();self.wfile.write(data)
 def do_GET(self):
  s=self.path.strip('/')
  if s not in logs:return self.reply({'error':'side required'})
  self.reply({'turn':b.battles[s].turn,'ended':b.ended,'winner':b.winner,'due':s in b.sides_to_move(),'request':b.battles[s].last_request,'events':logs[s],'out':str(out)})
 def do_POST(self):
  d=json.loads(self.rfile.read(int(self.headers.get('Content-Length',0))) or '{}')
  if self.path=='/shutdown':
   self.reply({'stopped':True});threading.Thread(target=server.shutdown).start();return
  try:
   choices=d['choices'];decisions.append({'turn':b.battles['p1'].turn,**d})
   r=b.step(choices)
   for s in logs:logs[s]+=r.lines[s]
   save();self.reply({'turn':b.battles['p1'].turn,'ended':b.ended,'winner':b.winner,'due':b.sides_to_move()})
  except Exception as e:self.reply({'error':repr(e)})
server=HTTPServer(('127.0.0.1',8767),H)
print('READY '+str(out),flush=True)
try:server.serve_forever()
finally:
 save();b.close();worker.close();server.server_close()
