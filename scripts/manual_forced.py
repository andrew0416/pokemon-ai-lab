"""Local turn controller: assistant chooses p1, fixed search policy chooses p2.
Only p1 protocol is exposed; p2 choices are committed before receiving p1 input.
"""
import argparse,json,random,threading
from datetime import datetime
from http.server import BaseHTTPRequestHandler,HTTPServer
from pathlib import Path
import numpy as np
import lab
lab.configure()
from vgc.rl.env import SimWorker,DirectBattle
from vgc.rl.agents import make_direct_agent
ROOT=lab.ROOT
OUT=ROOT/'runs'/('manual-'+datetime.now().strftime('%Y%m%d-%H%M%S'))
OUT.mkdir(parents=True)
worker=SimWorker(lab.SHOWDOWN)
battle=None
agent=None
pending=None
request={}
recent=[]
events=[]
calls=0
game=0
forced_preview=None
def absorb(lines):
    global request
    for line in lines:
        if line.startswith('|request|'):
            request=json.loads(line[len('|request|'):])
        else: recent.append(line)
    if battle:
        with (OUT/f'game-{game}.p1.log').open('a',encoding='utf-8') as f:
            f.write('\n'.join(lines)+'\n')
def prepare():
    global pending,calls
    while not battle.ended:
        sides=battle.sides_to_move()
        if 'p2' in sides and pending is None:
            calls+=1; random.seed(game*10000+calls);np.random.seed(game*10000+calls)
            pending=agent.choose(battle.battles['p2'])
            if battle.battles['p2'].turn == 0 and forced_preview:
                if not pending.startswith('team '):raise RuntimeError('Expected team preview command')
                pending=forced_preview
            if getattr(agent.player,'fallback_count',0):raise RuntimeError('Opponent fallback; stop game')
        if 'p1' in sides:return
        result=battle.step({'p2':pending});pending=None
        absorb(result.lines['p1']);agent.observe(battle.battle_id,result.lines['p2'])
def state():
    return {'game':game,'turn':battle.battles['p1'].turn,'ended':battle.ended,'winner':battle.winner,
            'request':request,'events':recent,'artifacts':str(OUT)}
class Handler(BaseHTTPRequestHandler):
    def log_message(self,*a):pass
    def do_GET(self):self.respond(state())
    def respond(self,obj,status=200):
        data=json.dumps(obj,ensure_ascii=False).encode('utf-8')
        self.send_response(status);self.send_header('Content-Type','application/json; charset=utf-8');self.end_headers();self.wfile.write(data)
    def do_POST(self):
        global battle,agent,pending,request,recent,calls,game,forced_preview
        try:
            data=json.loads(self.rfile.read(int(self.headers.get('Content-Length',0))) or b'{}')
            if self.path=='/new':
                if battle and not battle.ended:raise ValueError('Finish current game first')
                if battle:battle.close()
                game+=1;calls=0;pending=None;request={};recent=[]
                forced_preview=data.get('forced_preview')
                if forced_preview not in (None,'team 3214'):raise ValueError('Unsupported forced preview')
                opponent={'psyspam':'teams/psyspam-popular.json','sand':'teams/sand-owen.txt'}[data['opponent']]
                p1=lab.node_tool('team',ROOT/'teams/gravity-pyroar-trial.json',lab.FORMAT)
                p2=lab.node_tool('team',ROOT/opponent,lab.FORMAT)
                agent=make_direct_agent('vgc',p2,battle_format=lab.FORMAT)
                battle=DirectBattle.start(worker,f'manual{game}',p1,p2,battle_format=lab.FORMAT,seed=[9182,7431,5227,game])
                lab.dump(OUT/f'game-{game}.manifest.json',{'own_team':p1,'opponent_file':opponent,'opponent_policy':'vgc',
                    'strategy':data['strategy'],'forced_preview':forced_preview,'seed':[9182,7431,5227,game],'sources':lab.source_info(),
                    'knowledge':'Opponent archetype/sets were previously researched. Actual opponent choice and hidden state are not exposed.'})
                absorb(battle.last_lines['p1']);agent.observe(battle.battle_id,battle.last_lines['p2']);prepare()
                self.respond(state())
            elif self.path=='/act':
                if 'p1' not in battle.sides_to_move():raise ValueError('No player decision due')
                if not data.get('reason'):raise ValueError('Decision rationale required')
                events.append({'game':game,'turn':battle.battles['p1'].turn,'choice':data['choice'],'reason':data['reason']})
                lab.dump(OUT/'decisions.json',events)
                choices={'p1':data['choice']}
                if 'p2' in battle.sides_to_move():choices['p2']=pending
                recent=[]
                result=battle.step(choices);pending=None
                absorb(result.lines['p1']);agent.observe(battle.battle_id,result.lines['p2']);prepare()
                if battle.ended:lab.dump(OUT/f'game-{game}.result.json',{'winner':battle.winner,'turns':battle.battles['p1'].turn})
                self.respond(state())
            elif self.path=='/shutdown':
                self.respond({'stopped':True});threading.Thread(target=server.shutdown).start()
            else:self.respond({'error':'Unknown endpoint'},404)
        except Exception as e:self.respond({'error':repr(e)},400)
server=HTTPServer(('127.0.0.1',8766),Handler)
print('READY http://127.0.0.1:8766 '+str(OUT),flush=True)
try:server.serve_forever()
finally:
    if battle:battle.close()
    worker.close()
    server.server_close()
