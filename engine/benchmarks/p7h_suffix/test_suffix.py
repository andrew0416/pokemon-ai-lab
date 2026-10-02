"""Bounded synthetic protocol checks; no Rust/engine/network/Git invocation."""
from pathlib import Path,PurePosixPath
from contextlib import ExitStack
from types import SimpleNamespace
from unittest.mock import patch
import copy,json,os,shutil,sys,tempfile,unittest
import suffix_contract as h
import suffix_build as b
import suffix_gate as gate
import suffix_timing as r
import suffix_summary as s
from suffix_contract import c,base_ci
sys.path.insert(0,str(h.HERE.parent/'turn_distribution'))
import test_contract as fixtures
def write(path,value):
    path.parent.mkdir(parents=True,exist_ok=True);path.write_bytes((json.dumps(value,separators=(',',':'))+'\n').encode())
def testlog(names,filtered=0):
    return ('\n'.join('test '+name+' ... ok' for name in names)+'\ntest result: ok. '+str(len(names))+' passed; 0 failed; 0 ignored; 0 measured; '+str(filtered)+' filtered out;\n').encode()
def fixture_binding():
    value=c.strict_json((h.HERE/'source-binding.json').read_bytes())
    if value['source_sha']=='UNBOUND':
        value.update(source_sha='a'*40,changed_file_sha256={'engine/core/Cargo.toml':'a'*64},core_test_filter='p7h',core_tests=['turn::fixture::p7h_first','turn::fixture::p7h_second'])
    return value
class BuildTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup);self.ws=Path(self.tmp.name);self.bound=fixture_binding()
        stack=ExitStack();self.addCleanup(stack.close)
        stack.enter_context(patch.object(h,'binding',return_value=self.bound))
        stack.enter_context(patch.dict(os.environ,{'GITHUB_RUN_ID':'123','GITHUB_RUN_ATTEMPT':'1','GITHUB_OUTPUT':str(self.ws/'output')}))
        stack.enter_context(patch.object(base_ci,'environment',return_value={}))
        stack.enter_context(patch.object(h,'source_fingerprint',return_value=h.expected_source_proof(self.bound)))
        stack.enter_context(patch.object(h,'corpus',return_value=[]))
        stack.enter_context(patch.object(base_ci,'command',side_effect=lambda argv,cwd:'rustc 1.98.1 (fixture)' if argv[0]=='rustc' else 'c'*40))
        stack.enter_context(patch.object(b.subprocess,'run',side_effect=self.cargo))
        self.package=b.build(self.ws);self.digest=c.sha(h.package_root(self.ws)/'package.json')
    def cargo(self,argv,**kw):
        arm=Path(kw['env']['CARGO_TARGET_DIR']).name.removeprefix('target-p7h-')
        label,_,names=next(row for row in h.suites(self.bound,arm) if row[1]==argv)
        kw['stdout'].write(testlog(names,0 if label in ('joint_tests','benchmark_tests') else 1) if names else b'fresh synthetic build\n')
        root=h.target(self.ws,arm)/'release'
        for package,names in h.fingerprint_names(arm).items():
            for name in names:write(root/'.fingerprint'/(package+'-fixture')/name,{'features':json.dumps(h.features(arm) if package=='lab-engine' else []),'rustflags':['-Ctarget-cpu=x86-64']})
        for path in [root/h.TARGET,root/'deps'/(h.BROAD+'-fixture')]+([] if arm=='reference' else [root/h.BENCHMARK]):
            path.parent.mkdir(parents=True,exist_ok=True);path.write_bytes(('binary '+arm+' '+path.name).encode())
        if label=='broad_contract':write(Path(kw['env'][h.s.BROAD_MANIFEST_ENV]),h.broad_contract())
        return SimpleNamespace(returncode=0)
    def test_shared_eight_binaries_actual_producer_verifier_and_named_count(self):
        package,evidence=b.use_package(self.ws,self.digest)
        self.assertEqual(sum(len(v['binaries']) for v in evidence['arms'].values()),8)
        actual=sum(len(names) for arm in h.ARMS for label,argv,names in h.suites(self.bound,arm) if names)
        self.assertEqual(actual,h.named_count(self.bound));self.assertEqual(actual,evidence['fresh_named_test_executions'])
        self.assertEqual(sum(v.get('test_proof',{}).get('passed',0) for v in evidence['commands']),actual)
        self.assertEqual(h.features('reference'),h.features('off'));self.assertEqual(h.features('on'),h.features('off')+[h.FEATURE])
    def test_old_log_naming_bug_missing_named_proof_and_wrong_actual_feature_reject(self):
        root=h.package_root(self.ws);path=root/'build-receipt.json';original=c.strict_json(path.read_bytes())
        changed=copy.deepcopy(original);changed['commands'][0]['log']='reference_joint_tests.log';write(path,changed)
        with self.assertRaises(ValueError):b.verify_evidence(root,self.bound,str(self.ws))
        write(path,original);proof=original['arms']['on']['compiler_features'][0];fp=root/'fingerprints/on'/proof['path'];value=c.strict_json(fp.read_bytes());value['rustflags']=['-Ctarget-cpu=native'];write(fp,value)
        with self.assertRaises(ValueError):b.verify_evidence(root,self.bound,str(self.ws))
    def test_wrong_binary_source_attempt_and_package_digest_reject(self):
        with patch.dict(os.environ,{'GITHUB_RUN_ATTEMPT':'2'}):
            with self.assertRaises(ValueError):b.use_package(self.ws,self.digest)
        with self.assertRaises(ValueError):b.use_package(self.ws,'0'*64)
        path=h.package_bin(self.ws,'on','benchmark');path.write_bytes(b'other executable')
        with self.assertRaises(ValueError):b.use_package(self.ws,self.digest)
    def test_gate_producer_consumer_and_previous_source_or_incomplete_gate_reject(self):
        package,evidence=b.use_package(self.ws,self.digest,False);root=self.ws/'p7h-aggregate-results'
        summary={'status':'passed','full500_complete':True,'broad104_complete':True,'verified_full500_cases':500,'broad_fresh_named_tests':3,'issues':[],
          'source_sha':self.bound['source_sha'],'reference_sha':h.REFERENCE,'package_sha256':self.digest,'controller_sha':package['controller_sha'],
          'fresh_build_named_tests':h.named_count(self.bound),'shards':[{'index':i,'status':'passed','completed_cases':125} for i in range(4)]}
        write(root/'summary.json',summary)
        import suffix_accuracy
        with patch.object(suffix_accuracy,'aggregate',return_value=0):self.assertEqual(gate.issue(self.ws,self.digest),0)
        target=self.ws/gate.DIRECTORY;shutil.copytree(root,target);digest=c.sha(target/'gate.json')
        self.assertTrue(gate.verify(self.ws,digest,package,evidence,self.digest)['passed'])
        path=target/'gate.json';original=c.strict_json(path.read_bytes())
        mutations=[lambda v:v.update(source_sha=h.REFERENCE),lambda v:v.update(package_sha256='0'*64),lambda v:v.update(run_attempt='2'),lambda v:v.update(full500_complete=False),lambda v:v['features']['on'].remove(h.FEATURE)]
        for mutate in mutations:
            changed=copy.deepcopy(original);mutate(changed);write(path,changed)
            with self.assertRaises(ValueError):gate.verify(self.ws,c.sha(path),package,evidence,self.digest)
        write(path,original);summary['verified_full500_cases']=499;write(target/'summary.json',summary)
        with self.assertRaises(ValueError):gate.verify(self.ws,digest,package,evidence,self.digest)
    def test_incomplete_global_accuracy_cannot_issue_a_gate(self):
        import suffix_accuracy
        with patch.object(suffix_accuracy,'aggregate',return_value=1):
            self.assertEqual(gate.issue(self.ws,self.digest),1)
        self.assertFalse((self.ws/'p7h-aggregate-results/gate.json').exists())
class RawTests(unittest.TestCase):
    def setUp(self):
        self.bound=fixture_binding();self.patcher=patch.object(h,'binding',return_value=self.bound);self.patcher.start();self.addCleanup(self.patcher.stop)
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup);self.folder=Path(self.tmp.name)
        self.case={**fixtures.CASE,'scenario':'fixture.json','scenario_sha256':'0'*64}
        write(self.folder/'plan.json',fixtures.plan());self.golden={'sha256':c.sha(self.folder/'plan.json'),'bytes':(self.folder/'plan.json').stat().st_size}
        self.package={'source_sha':h.candidate_sha(),'build_workspace':'/home/runner/work/repo/repo'}
        self.evidence={'arms':{a:{'binaries':{'benchmark':{'sha256':str(i+1)*64}}} for i,a in enumerate(h.TIMING_ARMS)}}
        self.value={'case_id':self.case['id'],'source_sha':h.candidate_sha(),'plan_sha256':self.golden['sha256'],'status':'passed','descriptions':{},'rows':[]}
        for arm in h.TIMING_ARMS:
            stem='describe-'+arm;write(self.folder/(stem+'.stdout'),fixtures.plan());(self.folder/(stem+'.stderr')).write_bytes(b'')
            self.value['descriptions'][arm]=self.child(arm,stem,['--describe'])
        values=[111,222,100,50,50,100,100,60,60,100,100,70,70,100]
        remote=PurePosixPath(self.package['build_workspace'])/r.result_name(0)/'cases'/self.case['id']/'plan.json'
        for number,item in enumerate(s.schedule()):
            stem=f'measurement-{number:02d}-{item["arm"]}';value=fixtures.value();value['reference']['kernel_ns']=values[number]
            write(self.folder/(stem+'.stdout'),value);(self.folder/(stem+'.stderr')).write_bytes(b'')
            row=self.child(item['arm'],stem,['--plan',str(remote),'--sample-seeds',','.join(map(str,self.case['sample_seeds']))]);row.update(item,index=number);self.value['rows'].append(row)
        self.value['summary']=s.case_summary(self.case['id'],values)
    def child(self,arm,stem,extra):
        remote=PurePosixPath(self.package['build_workspace']);argv=[str(h.binary(remote,arm)),str(remote/'controller'/self.case['scenario']),'--joint-seed',str(self.case['joint_seed']),*extra]
        process={'argv':argv,'status':'ok','returncode':0,'wall_seconds':1.,'peak_rss_bytes':1000,'timeout_seconds':300,'rss_limit_bytes':h.RSS,'cpu_affinity':[0],'stdout_file':stem+'.stdout','stderr_file':stem+'.stderr'}
        row={'status':'ok','arm':arm,'case_id':self.case['id'],'source_sha':h.candidate_sha(),'plan_sha256':self.golden['sha256'],'binary_sha256':self.evidence['arms'][arm]['binaries']['benchmark']['sha256'],'argv':argv,'lab_environment':{},'process':process}
        for channel in ('stdout','stderr'):
            path=self.folder/(stem+'.'+channel);row[channel+'_sha256']=c.sha(path);row[channel+'_bytes']=path.stat().st_size
        return row
    def verify(self):return r.verify_case(self.folder,self.value,self.case,self.golden,self.package,self.evidence,0,0)
    def test_raw_timing_same_source_abba_warmup_exclusion(self):
        value,proofs=self.verify();self.assertEqual(value['median_block_on_over_off'],.6);self.assertEqual(len(proofs),13)
    def test_mixed_binary_source_seed_observer_order_and_timeout_reject(self):
        original=copy.deepcopy(self.value)
        mutations=[lambda v:v.update(source_sha=h.REFERENCE),lambda v:v.update(binary_sha256='f'*64),lambda v:v['argv'].__setitem__(3,'999'),lambda v:v['lab_environment'].update({'LAB_OBSERVER':'1'}),lambda v:v.update(arm='on'),lambda v:v['process'].update(wall_seconds=300.1)]
        for mutate in mutations:
            self.value=copy.deepcopy(original);mutate(self.value['rows'][2])
            with self.subTest(mutation=mutate),self.assertRaises(ValueError):self.verify()
    def test_missing_rows_and_forged_or_changed_raw_timing_reject(self):
        original=copy.deepcopy(self.value);self.value['rows'].pop()
        with self.assertRaises(ValueError):self.verify()
        self.value=copy.deepcopy(original);self.value['summary']['median_block_on_over_off']=.01
        with self.assertRaises(ValueError):self.verify()
        self.value=original;(self.folder/'measurement-02-off.stdout').write_bytes(b'{}\n')
        with self.assertRaises(ValueError):self.verify()
class ContractTests(unittest.TestCase):
    def test_exact500_disjoint_corpus_and_same_metrics_definition(self):
        rows=h.corpus(h.HERE.parents[3]);self.assertEqual(len(rows),500)
        self.assertEqual([len(rows[i::4]) for i in range(4)],[125]*4)
        values=[s.case_summary(f'opening-{i:04d}',[1,1]+[100,50 if i==0 else 100,50 if i==0 else 100,100]*3) for i in range(4)]
        result=s.whole_summary(values,[v['case_id'] for v in values]);self.assertLess(result['case_equal_weight_geometric_mean_on_over_off'],1)
        with self.assertRaises(ValueError):s.whole_summary(values[:3],[v['case_id'] for v in values])
    def test_unbound_source_refuses_publication_or_exact_bound_contract_validates(self):
        if c.strict_json((h.HERE/'source-binding.json').read_bytes())['source_sha']=='UNBOUND':
            with self.assertRaisesRegex(ValueError,'Source not frozen'):h.binding()
        else:self.assertEqual(h.binding()['reference_sha'],h.REFERENCE)
if __name__=='__main__':unittest.main()
