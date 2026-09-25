"""Bounded loopback server smoke; stop the process tree we started."""
import json
import shutil
import subprocess
import time
from pathlib import Path
from websockets.sync.client import connect
root=Path(__file__).resolve().parents[1]
logpath=root/'runs/server-smoke.log'
with logpath.open('w',encoding='utf-8') as log:
    process=subprocess.Popen([shutil.which('node'),'pokemon-showdown','start','8765','--no-security'],
        cwd=root/'vendor/pokemon-showdown',stdout=log,stderr=subprocess.STDOUT)
    try:
        deadline=time.monotonic()+25
        while True:
            if process.poll() is not None:
                raise RuntimeError('Server exited: '+logpath.read_text(encoding='utf-8'))
            try:
                with connect('ws://127.0.0.1:8765/showdown/websocket',open_timeout=1) as ws:
                    message=ws.recv(timeout=3)
                    assert '|updateuser|' in message or '|challstr|' in message, message
                break
            except (OSError,TimeoutError):
                if time.monotonic()>deadline: raise
                time.sleep(.3)
        result={'status':'pass','url':'ws://127.0.0.1:8765/showdown/websocket','stopped_after_check':True}
    finally:
        subprocess.run(['taskkill','/PID',str(process.pid),'/T','/F'],capture_output=True,check=False)
        process.wait(timeout=10)
(root/'runs/server-smoke.json').write_text(json.dumps(result,indent=2),encoding='utf-8')
print(json.dumps(result,indent=2))
