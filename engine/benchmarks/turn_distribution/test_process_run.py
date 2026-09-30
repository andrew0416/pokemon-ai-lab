"""Synthetic process/resource checks; no Rust or performance measurement."""
from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest
from unittest.mock import patch
import process_run as p

class ChildProcessTests(unittest.TestCase):
    def simulate(self,mode):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);proc=SimpleNamespace(pid=123,returncode=None)
            usage=SimpleNamespace(ru_maxrss=100,ru_utime=0.01,ru_stime=0.02)
            if mode=='peak':usage.ru_maxrss=7*1024**2
            statuses=[(0,0,None),(123,256 if mode=='nonzero' else 9 if mode in ('timeout','rss') else 0,usage)]
            fakeos=SimpleNamespace(name='posix',WNOHANG=1,wait4=lambda *args:statuses.pop(0),
                sched_getaffinity=lambda _: {1,2},sched_setaffinity=lambda *args:None,
                waitstatus_to_exitcode=lambda status: -9 if status==9 else 1 if status==256 else 0,
                killpg=lambda *args:None)
            clock=iter([0,61 if mode=='timeout' else 1,62 if mode=='timeout' else 2])
            with (patch.object(p,'os',fakeos),patch.object(p.subprocess,'Popen',return_value=proc) as popen,
                    patch.object(p,'rss_bytes',return_value=(7*1024**3 if mode=='rss' else 100,100)),
                    patch.object(p.time,'sleep'),patch.object(p.time,'monotonic',side_effect=lambda:next(clock))):
                result=p.run(['frozen-binary'],root,{},root/'case',cpu=1)
            self.assertEqual(popen.call_args.kwargs['stdin'],p.subprocess.DEVNULL)
            self.assertTrue(popen.call_args.kwargs['start_new_session'])
            self.assertEqual(result['status'],{'timeout':'timeout','rss':'rss_limit','peak':'rss_limit',
                'nonzero':'nonzero_exit','ok':'ok'}[mode])
            self.assertEqual(result['cpu_affinity'],[1])
            self.assertTrue((root/'case.stdout').exists());self.assertTrue((root/'case.stderr').exists())
            self.assertEqual(result['virtual_address_space_limit'],None)
    def test_terminal_statuses_and_resources_are_not_conflated(self):
        for mode in ('ok','timeout','rss','peak','nonzero'):
            with self.subTest(mode=mode):self.simulate(mode)

if __name__=='__main__':unittest.main()
