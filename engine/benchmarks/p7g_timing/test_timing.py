"""Pure Python protocol checks. All external builds/children are mocked, never executed."""
from pathlib import Path,PurePosixPath
from contextlib import ExitStack
from types import SimpleNamespace
from unittest.mock import patch
import copy,json,os,sys,tempfile,time,unittest
import timing_contract as h
import timing_build as b
import timing_run as r
import timing_summary as s
from timing_contract import c,base_ci
sys.path.insert(0,str(h.HERE.parent/'turn_distribution'))
import test_contract as fixtures
def write(path,value):
    path.parent.mkdir(parents=True,exist_ok=True);path.write_bytes((json.dumps(value,separators=(',',':'))+'\n').encode())
def testlog(names):
    return ('\n'.join('test '+n+' ... ok' for n in names)+'\ntest result: ok. '+str(len(names))+' passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n').encode()
class BuildTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup);self.ws=Path(self.tmp.name)
        stack=ExitStack();self.addCleanup(stack.close)
        stack.enter_context(patch.dict(os.environ,{'GITHUB_RUN_ID':'123','GITHUB_RUN_ATTEMPT':'1','GITHUB_OUTPUT':str(self.ws/'output')}))
        stack.enter_context(patch.object(base_ci,'environment',return_value={}))
        stack.enter_context(patch.object(h,'source_proof',return_value=h.expected_source_proof(h.binding())))
        stack.enter_context(patch.object(h,'corpus',return_value=[]))
        stack.enter_context(patch.object(base_ci,'command',side_effect=lambda argv,cwd:'rustc 1.98.1 (fixture)' if argv[0]=='rustc' else 'c'*40))
        stack.enter_context(patch.object(b.subprocess,'run',side_effect=self.cargo))
        self.package=b.build(self.ws);self.digest=c.sha(h.package_root(self.ws)/'package.json')
    def cargo(self,argv,**kw):
        arm=Path(kw['env']['CARGO_TARGET_DIR']).name.removeprefix('target-p7g-')
        label,_,names=next(row for row in h.commands(arm) if row[1]==argv)
        kw['stdout'].write(testlog(names) if names else b'fresh build\n')
        root=h.target(self.ws,arm)/'release'
        for package,names in h.fingerprint_names(arm).items():
            for name in names:write(root/'.fingerprint'/(package+'-mock')/name,{'features':json.dumps(h.features(arm) if package=='lab-engine' else []),'rustflags':['-Ctarget-cpu=x86-64']})
        (root/h.TARGET).write_bytes(('binary '+arm).encode());return SimpleNamespace(returncode=0)
    def test_real_build_verify_consume_contract_28_named_checks(self):
        package,evidence=b.use_package(self.ws,self.digest)
        self.assertEqual(package,self.package);self.assertEqual(evidence['fresh_named_test_executions'],28)
        self.assertEqual(sum(v.get('test_proof',{}).get('passed',0) for v in evidence['commands']),28)
    def test_producer_log_naming_and_missing_names_reject(self):
        root=h.package_root(self.ws);path=root/'build-receipt.json';original=c.strict_json(path.read_bytes())
        changed=copy.deepcopy(original);changed['commands'][0]['log']='off_benchmark_tests.log';write(path,changed)
        with self.assertRaises(ValueError):b.verify_evidence(root,str(self.ws))
        write(path,original);(root/original['commands'][0]['log']).write_bytes(b'test result: ok. 7 passed; 0 failed;\n')
        with self.assertRaises(ValueError):b.verify_evidence(root,str(self.ws))
    def test_actual_binary_fingerprint_and_package_members_fail_closed(self):
        root=h.package_root(self.ws);binary=h.binary(self.ws,'on');original=binary.read_bytes();binary.write_bytes(b'other')
        with self.assertRaises(ValueError):b.use_package(self.ws,self.digest)
        binary.write_bytes(original);path=root/'extra';path.write_bytes(b'extra')
        with self.assertRaises(ValueError):b.use_package(self.ws,self.digest)
        path.unlink();receipt=c.strict_json((root/'build-receipt.json').read_bytes());proof=receipt['arms']['on']['compiler_features'][0];fp=root/'fingerprints/on'/proof['path'];data=c.strict_json(fp.read_bytes());data['rustflags']=['-Ctarget-cpu=native'];write(fp,data)
        with self.assertRaises(ValueError):b.verify_evidence(root,str(self.ws))
    def test_wrong_source_attempt_controller_and_digest_reject(self):
        with patch.dict(os.environ,{'GITHUB_RUN_ATTEMPT':'2'}):
            with self.assertRaises(ValueError):b.use_package(self.ws,self.digest)
        with self.assertRaises(ValueError):b.verify_package(h.package_root(self.ws),self.digest,'a'*40,'123','1',str(self.ws))
        with self.assertRaises(ValueError):b.use_package(self.ws,'0'*64)
        path=h.package_root(self.ws)/'build-receipt.json';v=c.strict_json(path.read_bytes());v['source_sha']='0'*40;write(path,v)
        with self.assertRaises(ValueError):b.verify_evidence(h.package_root(self.ws),str(self.ws))
class RawTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup);self.folder=Path(self.tmp.name)
        self.case={**fixtures.CASE,'scenario':'fixture.json','scenario_sha256':'0'*64}
        write(self.folder/'plan.json',fixtures.plan());self.golden={'sha256':c.sha(self.folder/'plan.json'),'bytes':(self.folder/'plan.json').stat().st_size}
        self.package={'source_sha':h.SOURCE,'build_workspace':'/home/runner/work/repo/repo'}
        self.evidence={'arms':{a:{'binary':{'sha256':str(i+1)*64}} for i,a in enumerate(h.ARMS)}}
        self.value={'case_id':self.case['id'],'source_sha':h.SOURCE,'plan_sha256':self.golden['sha256'],'status':'passed','descriptions':{},'rows':[]}
        for arm in h.ARMS:
            stem='describe-'+arm;write(self.folder/(stem+'.stdout'),fixtures.plan());(self.folder/(stem+'.stderr')).write_bytes(b'')
            self.value['descriptions'][arm]=self.child(arm,stem,['--describe'])
        values=[111,222,100,50,50,100,100,60,60,100,100,70,70,100]
        remote=PurePosixPath(self.package['build_workspace'])/r.result_name(0)/'cases'/self.case['id']/'plan.json'
        for number,item in enumerate(s.schedule()):
            stem=f'measurement-{number:02d}-{item["arm"]}';v=fixtures.value();v['reference']['kernel_ns']=values[number]
            write(self.folder/(stem+'.stdout'),v);(self.folder/(stem+'.stderr')).write_bytes(b'')
            row=self.child(item['arm'],stem,['--plan',str(remote),'--sample-seeds',','.join(map(str,self.case['sample_seeds']))]);row.update(item,index=number);self.value['rows'].append(row)
        self.value['summary']=s.case_summary(self.case['id'],values)
    def child(self,arm,stem,extra):
        remote=PurePosixPath(self.package['build_workspace']);argv=[str(h.binary(remote,arm)),str(remote/'controller'/self.case['scenario']),'--joint-seed',str(self.case['joint_seed']),*extra]
        p={'argv':argv,'status':'ok','returncode':0,'wall_seconds':1.,'peak_rss_bytes':1000,'timeout_seconds':300,'rss_limit_bytes':h.RSS,'cpu_affinity':[0],'stdout_file':stem+'.stdout','stderr_file':stem+'.stderr'}
        row={'status':'ok','arm':arm,'case_id':self.case['id'],'source_sha':h.SOURCE,'plan_sha256':self.golden['sha256'],'binary_sha256':self.evidence['arms'][arm]['binary']['sha256'],'argv':argv,'lab_environment':{},'process':p}
        for channel in ('stdout','stderr'):
            path=self.folder/(stem+'.'+channel);row[channel+'_sha256']=c.sha(path);row[channel+'_bytes']=path.stat().st_size
        return row
    def verify(self):return r.verify_case(self.folder,self.value,self.case,self.golden,self.package,self.evidence,0,0)
    def test_recompute_real_raw_ABBA_ignores_warmup_and_uses_block_ratios(self):
        result,checks=self.verify();self.assertEqual(result['median_block_on_over_off'],.6);self.assertEqual(result['off_timed_kernel_ns_sum'],600);self.assertEqual(len(checks),13)
    def test_mixed_binary_source_seed_environment_order_and_late_success_reject(self):
        original=copy.deepcopy(self.value)
        mutations=[lambda v:v.update(binary_sha256='f'*64),lambda v:v.update(source_sha='0'*40),lambda v:v['argv'].__setitem__(3,'999'),lambda v:v['lab_environment'].update({'LAB_OBSERVER':'1'}),lambda v:v.update(arm='on'),lambda v:v['process'].update(wall_seconds=300.1),lambda v:v['process'].update(peak_rss_bytes=h.RSS+1),lambda v:v['process'].update(cpu_affinity=[1])]
        for mutate in mutations:
            self.value=copy.deepcopy(original);mutate(self.value['rows'][2])
            with self.subTest(mutation=mutate),self.assertRaises(ValueError):self.verify()
    def test_missing_rows_raw_bytes_or_forged_ratio_reject(self):
        original=copy.deepcopy(self.value);self.value['rows'].pop()
        with self.assertRaises(ValueError):self.verify()
        self.value=copy.deepcopy(original);self.value['summary']['median_block_on_over_off']=.01
        with self.assertRaises(ValueError):self.verify()
        self.value=original;(self.folder/'measurement-02-off.stdout').write_bytes(b'{}\n')
        with self.assertRaises(ValueError):self.verify()
    def test_refused_result_never_becomes_kernel_ratio(self):
        self.value['rows'][3]['status']='timeout'
        with self.assertRaises(ValueError):self.verify()
    def test_scalar_semantic_drift_not_hidden_by_valid_timing(self):
        path=self.folder/'measurement-03-on.stdout';v=c.line(path);v['samples'][0]['seed']+=1;write(path,v)
        row=self.value['rows'][3];row['stdout_sha256']=c.sha(path);row['stdout_bytes']=path.stat().st_size
        with self.assertRaises(ValueError):self.verify()
class SummaryTests(unittest.TestCase):
    def test_frozen_accuracy_and_exact500_partition(self):
        binding=h.binding();self.assertEqual(binding['source_sha'],h.SOURCE)
        rows=h.corpus(h.HERE.parents[3]);self.assertEqual(len(rows),500)
        self.assertEqual([len(rows[i::4]) for i in range(4)],[125]*4)
        self.assertEqual(len({v['id'] for i in range(4) for v,g in rows[i::4]}),500)
    def test_equal_case_geomean_differs_from_heavy_weighted_workload(self):
        cases=[]
        for i,ratio in enumerate((.5,2.,1.,1.)):
            base=10000 if i==0 else 100
            v=s.case_summary(f'opening-{i:04d}',[1,1]+[base,int(base*ratio),int(base*ratio),base]*3);cases.append(v)
        result=s.whole_summary(cases,[v['case_id'] for v in cases])
        self.assertEqual(result['case_equal_weight_geometric_mean_on_over_off'],1)
        self.assertLess(result['observed_mixed_runner_workload']['on_over_off'],.6)
        self.assertEqual(result['case_ratio_p90'],1.7000000000000002)
        self.assertEqual(sum(result['bands'].values()),4)
    def test_missing_duplicate_nonfinite_and_wrong_headline_membership_reject(self):
        values=[s.case_summary(f'opening-{i:04d}',[1]*14) for i in range(4)];ids=[v['case_id'] for v in values]
        for changed in (values[:3],values+[values[0]],list(reversed(values))):
            with self.assertRaises(ValueError):s.whole_summary(changed,ids)
        values[0]['median_block_on_over_off']=float('nan')
        with self.assertRaises(ValueError):s.whole_summary(values,ids)
    def test_budget_never_calls_engine_and_missing_artifacts_have_no_headline(self):
        with patch.object(h.tail_process,'run',side_effect=AssertionError('no process')):
            row=r.execute(Path('/unused'),{},'off',{'id':'opening-0000'},{'sha256':'x'},Path('/unused'),[],time.monotonic(),0)
        self.assertEqual(row['status'],'not_run_budget')
        with tempfile.TemporaryDirectory() as tmp:
            ws=Path(tmp);package={'controller_sha':'c'*40}
            with patch.object(b,'use_package',return_value=(package,{})),patch.object(h,'corpus',return_value=[({'id':f'opening-{i:04d}'},{}) for i in range(500)]),patch.dict(os.environ,{'GITHUB_RUN_ID':'123'}):
                self.assertEqual(r.aggregate(ws,'a'*64),1)
            result=c.strict_json((ws/'p7g-aggregate-results/summary.json').read_bytes())
            self.assertFalse(result['full500_complete']);self.assertIsNone(result['headline']);self.assertEqual(len(result['issues']),4)
if __name__=='__main__':unittest.main()
