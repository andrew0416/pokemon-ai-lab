"""Bounded Linux child execution; whole-process resources are not kernel timings."""
import os
from pathlib import Path
import signal
import subprocess
import time

POLL_SECONDS=0.01
KILL_SIGNAL=getattr(signal,'SIGKILL',9)  # Windows only runs synthetic controller tests.

def rss_bytes(pid):
    """Current/HWM RSS of this Rust process, including its threads."""
    try:
        lines=Path(f'/proc/{pid}/status').read_text().splitlines()
    except (FileNotFoundError,ProcessLookupError):
        return 0,0
    fields={line.split(':',1)[0]:line.split(':',1)[1].split() for line in lines if ':' in line}
    def read(name):
        values=fields.get(name,['0','kB'])
        if len(values)!=2 or values[1]!='kB':raise ValueError('Unexpected Linux RSS units')
        return int(values[0])*1024
    return read('VmRSS'),read('VmHWM')

def run(argv,cwd,env,stem,*,cpu,timeout_seconds=300,rss_limit_bytes=6*1024**3):
    if os.name!='posix' or not hasattr(os,'wait4') or not hasattr(os,'sched_setaffinity'):
        raise ValueError('Measurement requires Linux affinity and wait4 resource accounting')
    if timeout_seconds!=300 or rss_limit_bytes!=6*1024**3:
        raise ValueError('Unexpected frozen process resource bounds')
    if type(cpu) is not int or cpu not in os.sched_getaffinity(0):
        raise ValueError('Measurement CPU is not available')
    parent_affinity=set(os.sched_getaffinity(0))
    monitor_affinity={min(parent_affinity-{cpu})} if parent_affinity-{cpu} else parent_affinity
    stem=Path(stem);stdout=stem.with_suffix('.stdout');stderr=stem.with_suffix('.stderr')
    started=time.monotonic();peak=0;reason=None;proc=None;usage=None
    with stdout.open('xb') as out,stderr.open('xb') as err:
        try:
            proc=subprocess.Popen(argv,cwd=cwd,env=env,stdout=out,stderr=err,
                stdin=subprocess.DEVNULL,start_new_session=True,
                preexec_fn=lambda:os.sched_setaffinity(0,{cpu}))
            os.sched_setaffinity(0,monitor_affinity)
            while True:
                pid,status,usage=os.wait4(proc.pid,os.WNOHANG)
                if pid:
                    proc.returncode=os.waitstatus_to_exitcode(status)
                    break
                current,high=rss_bytes(proc.pid);peak=max(peak,current,high)
                elapsed=time.monotonic()-started
                if reason is None and (peak>rss_limit_bytes or elapsed>=timeout_seconds):
                    reason='rss_limit' if peak>rss_limit_bytes else 'timeout'
                    try:os.killpg(proc.pid,KILL_SIGNAL)
                    except ProcessLookupError:pass
                time.sleep(POLL_SECONDS)
        except BaseException:
            if proc is not None and proc.returncode is None:
                try:os.killpg(proc.pid,KILL_SIGNAL)
                except ProcessLookupError:pass
                _,status,usage=os.wait4(proc.pid,0)
                proc.returncode=os.waitstatus_to_exitcode(status)
            raise
        finally:
            os.sched_setaffinity(0,parent_affinity)
    elapsed=time.monotonic()-started
    peak=max(peak,int(usage.ru_maxrss)*1024)
    if reason is None:
        reason='rss_limit' if peak>rss_limit_bytes else 'ok' if proc.returncode==0 else 'nonzero_exit'
    return {'argv':list(argv),'returncode':proc.returncode,'status':reason,'wall_seconds':elapsed,
            'user_cpu_seconds':usage.ru_utime,'system_cpu_seconds':usage.ru_stime,
            'peak_rss_bytes':peak,'cpu_affinity':[cpu],
            'monitor_cpu_affinity':sorted(monitor_affinity),
            'timeout_seconds':timeout_seconds,'rss_limit_bytes':rss_limit_bytes,
            'rss_enforcement':'/proc polling every 10ms; terminal wait4 high-water mark also checked',
            'rss_limit_may_overshoot_between_polls':True,'virtual_address_space_limit':None,
            'stdout_file':stdout.name,'stderr_file':stderr.name}
