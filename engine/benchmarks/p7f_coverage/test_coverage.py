"""Pure Python producer/verifier regressions; never invoke Rust, an engine or the network."""
from pathlib import Path, PurePosixPath
from types import SimpleNamespace
from unittest.mock import patch
from contextlib import ExitStack
import copy,hashlib,json,os,sys,tempfile,time,unittest
import coverage_contract as h
import coverage_build as b
import coverage_run as r
sys.path.insert(0,str(h.HERE.parent/'p1e2_adoption'))
import test_joint as fixture_joint
import test_broad as fixture_broad
from coverage_contract import c,base_ci

def write(path,value):
    path.parent.mkdir(parents=True,exist_ok=True)
    path.write_bytes((json.dumps(value,sort_keys=True,indent=2)+'\n').encode())
def test_log(names,filtered=0):
    return ('\n'.join('test '+n+' ... ok' for n in names)+'\ntest result: ok. '+str(len(names))+' passed; 0 failed; 0 ignored; 0 measured; '+str(filtered)+' filtered out;\n').encode()
def source_proof(bound):
    return {'source_sha':bound['source_sha'],'source_parent':h.RUNTIME,'runtime_sha':h.RUNTIME,'reference_sha':h.REFERENCE,'test_only_file_sha256':bound['test_only_file_sha256'],'runtime_bytes_unchanged':True,'original_producers':h.prior.FROZEN}

class BuildTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup);self.ws=Path(self.tmp.name);self.bound=h.binding()
        stack=ExitStack();self.addCleanup(stack.close)
        stack.enter_context(patch.dict(os.environ,{'GITHUB_RUN_ID':'123','GITHUB_RUN_ATTEMPT':'1','GITHUB_OUTPUT':str(self.ws/'output')}))
        stack.enter_context(patch.object(base_ci,'environment',return_value={}))
        stack.enter_context(patch.object(h,'source_fingerprint',return_value=source_proof(self.bound)))
        stack.enter_context(patch.object(h,'corpus',return_value=[]))
        stack.enter_context(patch.object(base_ci,'command',side_effect=lambda argv,cwd:'rustc 1.98.1 (synthetic)' if argv[0]=='rustc' else 'c'*40))
        stack.enter_context(patch.object(b.subprocess,'run',side_effect=self.cargo))
        self.package=b.build(self.ws);self.digest=c.sha(h.package_root(self.ws)/'package.json')
    def cargo(self,argv,**kw):
        arm=Path(kw['env']['CARGO_TARGET_DIR']).name.removeprefix('target-p7f-')
        label,_,names=next(row for row in h.suites(self.bound,arm) if row[1]==argv)
        kw['stdout'].write(test_log(names,0 if label=='joint_tests' else 1) if names else b'Fresh synthetic build\n')
        root=h.target(self.ws,arm)/'release'
        for package,names_ in h.fingerprint_names(arm).items():
            for name in names_:
                write(root/'.fingerprint'/(package+'-synthetic')/name,{'features':json.dumps(h.features(arm) if package=='lab-engine' else []),'rustflags':['-Ctarget-cpu=x86-64']})
        if arm!='edge':
            for path in (root/h.TARGET,root/'deps'/(h.BROAD+'-fixture')):
                path.parent.mkdir(parents=True,exist_ok=True);path.write_bytes(('binary '+arm+' '+path.name).encode())
        if label=='broad_contract':write(Path(kw['env'][h.s.BROAD_MANIFEST_ENV]),h.broad_contract())
        return SimpleNamespace(returncode=0)
    def repack(self):
        root=h.package_root(self.ws);p=c.strict_json((root/'package.json').read_bytes())
        p['file_sha256']={f.relative_to(root).as_posix():c.sha(f) for f in root.rglob('*') if f.is_file() and f.name!='package.json'}
        write(root/'package.json',p);return c.sha(root/'package.json')
    def test_actual_producer_verifier_and_consumer_84_named_checks(self):
        package,evidence=b.use_package(self.ws,self.digest)
        self.assertEqual(evidence['fresh_named_test_executions'],84)
        self.assertEqual(sum(x.get('test_proof',{}).get('passed',0) for x in evidence['commands']),84)
        self.assertEqual(package,self.package)
    def test_build_log_naming_regression_rejects_old_underscore_variant(self):
        root=h.package_root(self.ws);receipt=c.strict_json((root/'build-receipt.json').read_bytes())
        receipt['commands'][0]['log']=receipt['commands'][0]['log'].replace('-','_',1);write(root/'build-receipt.json',receipt)
        with self.assertRaisesRegex(ValueError,'Fresh log'):b.verify_evidence(root,self.bound,str(self.ws))
    def test_changed_binary_rejected_even_with_repacked_inventory(self):
        h.package_bin(self.ws,'on','joint').write_bytes(b'other binary')
        with self.assertRaisesRegex(ValueError,'Executable bytes'):b.use_package(self.ws,self.repack())
    def test_missing_package_member_and_wrong_digest_reject(self):
        with self.assertRaises(ValueError):b.use_package(self.ws,'0'*64)
        h.package_bin(self.ws,'off','broad').unlink()
        with self.assertRaisesRegex(ValueError,'package members'):b.use_package(self.ws,self.digest)
    def test_mixed_attempt_source_and_actual_features_reject(self):
        with patch.dict(os.environ,{'GITHUB_RUN_ATTEMPT':'2'}):
            with self.assertRaisesRegex(ValueError,'another run'):b.use_package(self.ws,self.digest)
        root=h.package_root(self.ws);path=root/'build-receipt.json';value=c.strict_json(path.read_bytes())
        for mutation in (lambda v:v['arms']['on'].update(source_sha=h.REFERENCE),lambda v:v.update(source_sha=h.REFERENCE)):
            changed=copy.deepcopy(value);mutation(changed);write(path,changed)
            with self.assertRaises(ValueError):b.verify_evidence(root,self.bound,str(self.ws))
        write(path,value)
        proof=value['arms']['on']['compiler_features'][0];file=root/'fingerprints/on'/proof['path'];data=c.strict_json(file.read_bytes());data['rustflags']=['-Ctarget-cpu=native'];write(file,data)
        with self.assertRaises(ValueError):b.verify_evidence(root,self.bound,str(self.ws))
    def test_named_log_false_pass_missing_and_ignored_reject(self):
        root=h.package_root(self.ws);row=c.strict_json((root/'build-receipt.json').read_bytes())['commands'][0]
        path=root/row['log'];path.write_bytes(b'test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n')
        with self.assertRaises(ValueError):b.verify_evidence(root,self.bound,str(self.ws))

class RawCaseTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup);self.root=Path(self.tmp.name)
        self.package={'source_sha':h.binding()['source_sha'],'build_workspace':'/home/runner/work/repo/repo'}
        self.evidence={'arms':{a:{'source_sha':h.source_sha(h.binding(),a),'binaries':{'joint':{'sha256':str(i+1)*64},'broad':{'sha256':str(i+4)*64}}} for i,a in enumerate(h.ARMS)}}
        self.use={'actual_cpu_affinity':[0]};self.case={'id':'opening-0000','joint_seed':7,'scenario':'fixture.json','sample_seeds':[1,2,3,4,5]}
        self.directory=self.root/'case';self.directory.mkdir();self.row={'case_id':self.case['id'],'source_sha':self.package['source_sha'],'status':'passed','arms':{},'comparisons':{}}
        remote=PurePosixPath(self.package['build_workspace']);remote_case=remote/r.run_name('full500',0)/'cases'/self.case['id']
        args=[str(remote/'controller'/self.case['scenario']),'--joint-seed','7']
        fixture=fixture_joint.JointExportTests()
        for arm in h.ARMS:
            folder=self.directory/arm;folder.mkdir();plan=fixture.write(folder/'export')
            (folder/'describe.stdout').write_bytes(plan);(folder/'describe.stderr').write_bytes(b'')
            (folder/'execution.stdout').write_bytes((folder/'export/manifest.json').read_bytes());(folder/'execution.stderr').write_bytes(b'')
            actual=h.compare_joint.read_export(folder/'export',plan)
            desc=self.child(arm,folder,'describe',args+['--describe'])
            tail=args+['--plan',str(remote_case/'reference/describe.stdout'),'--output-dir',str(remote_case/arm/'export'),'--max-rows',str(h.compare_joint.MAX_ROWS),'--max-bytes',str(h.compare_joint.MAX_BYTES)]
            execution=self.child(arm,folder,'execution',tail)
            self.row['arms'][arm]={'status':'complete','description':desc,'execution':execution,'manifest':actual['manifest'],'file_sha256':actual['file_sha256'],'joint_rows':actual['rows'],'raw_mass':actual['mass'],'export_files':h.prior.inherited_tail.inventory(folder/'export')}
        self.golden={'sha256':hashlib.sha256(plan).hexdigest(),'bytes':len(plan)};self.row['plan_sha256']=self.golden['sha256']
        for arm in ('off','on'):
            proof=h.compare_joint.compare_exports(self.directory/'reference/export',self.directory/arm/'export',plan)
            if arm=='off':proof['reference_off_payload_bytes_exact']=True
            self.row['comparisons']['reference_vs_'+arm]=proof
    def child(self,arm,folder,label,tail,kind='joint',lab=None):
        argv=[str(h.package_bin(PurePosixPath(self.package['build_workspace']),arm,kind)),*tail]
        process={'argv':argv,'status':'ok','returncode':0,'wall_seconds':1.,'peak_rss_bytes':1000,'rss_limit_bytes':h.RSS,'timeout_seconds':300,'cpu_affinity':[0],'stdout_file':label+'.stdout','stderr_file':label+'.stderr'}
        row={'arm':arm,'source_sha':self.evidence['arms'][arm]['source_sha'],'binary_sha256':self.evidence['arms'][arm]['binaries'][kind]['sha256'],'status':'ok','argv':argv,'lab_environment':lab or {},'process':process}
        for channel in ('stdout','stderr'):
            path=folder/(label+'.'+channel);row[channel+'_sha256']=c.sha(path);row[channel+'_bytes']=path.stat().st_size
        return row
    def verify(self):
        with patch.object(c,'description'):
            return r.verify_case_files(self.directory,self.row,self.case,self.golden,self.package,self.evidence,self.use,0)
    def test_raw_full_support_recomputed_and_off_payload_byte_identity(self):
        self.assertTrue(self.verify()['passed'])
    def test_mixed_source_binary_seed_environment_and_late_success_reject(self):
        original=copy.deepcopy(self.row)
        mutations=[lambda e:e.update(source_sha=h.REFERENCE),lambda e:e.update(binary_sha256='f'*64),
          lambda e:e['argv'].__setitem__(3,'999'),lambda e:e['lab_environment'].update({'LAB_ENGINE_FACTORED':'0'}),
          lambda e:e['process'].update(wall_seconds=300.01),lambda e:e['process'].update(peak_rss_bytes=h.RSS+1),
          lambda e:e['process'].update(cpu_affinity=[0,1]),lambda e:e['process'].update(status='timeout')]
        for mutate in mutations:
            self.row=copy.deepcopy(original);mutate(self.row['arms']['on']['execution'])
            with self.subTest(mutation=mutate),self.assertRaises(ValueError):self.verify()
    def test_changed_hidden_state_and_last_reserve_rejected_from_raw(self):
        path=self.directory/'on/export/dictionary.json';data=c.strict_json(path.read_bytes());data['entries'][0]=data['entries'][0].replace('hidden: 1','hidden: 2');write(path,data)
        with self.assertRaises(ValueError):self.verify()
    def test_derived_claim_forgery_rejected(self):
        self.row['comparisons']['reference_vs_on']['max_abs_probability_error']=.1
        with self.assertRaisesRegex(ValueError,'Raw full joint'):self.verify()
    def test_missing_manifest_and_incomplete_arm_cannot_pass(self):
        self.row['arms']['on']['status']='timeout'
        with self.assertRaises(ValueError):self.verify()
        self.row['arms']['on']['status']='complete';(self.directory/'on/export/manifest.json').unlink()
        with self.assertRaises(ValueError):self.verify()
    def test_off_probabilities_within_tolerance_still_require_exact_bytes(self):
        path=self.directory/'off/export/joint.bin';data=bytearray(path.read_bytes());first=list(h.compare_joint.ROW.unpack(data[:36]));first[-1]+=1e-14;data[:36]=h.compare_joint.ROW.pack(*first);last=list(h.compare_joint.ROW.unpack(data[36:]));last[-1]-=1e-14;data[36:]=h.compare_joint.ROW.pack(*last);path.write_bytes(data)
        actual=h.compare_joint.read_export(path.parent,self.directory/'reference/describe.stdout')
        entry=self.row['arms']['off'];entry['file_sha256']=actual['file_sha256'];entry['export_files']=h.prior.inherited_tail.inventory(path.parent)
        self.row['comparisons']['reference_vs_off']=h.compare_joint.compare_exports(self.directory/'reference/export',path.parent,self.directory/'reference/describe.stdout')
        self.row['comparisons']['reference_vs_off']['reference_off_payload_bytes_exact']=True
        with self.assertRaisesRegex(ValueError,'payload bytes'):self.verify()
    def test_broad_child_invocation_and_records_path_bound(self):
        folder=self.directory/'on';lab={**h.s.SCOPE_ENV,h.s.BROAD_ENV:self.package['build_workspace']+'/p7f-broad-results/on/records.jsonl'}
        tail=['p1e2_full_state_corpus','--exact','--test-threads=1','--show-output'];row=self.child('on',folder,'execution',tail,'broad',lab);row['status']='complete'
        r.verify_process(row,folder,'execution','on','broad',tail,lab,self.package,self.evidence,self.use,'complete')
        row['lab_environment']={**lab,h.s.BROAD_ENV:'/wrong/records.jsonl'}
        with self.assertRaises(ValueError):r.verify_process(row,folder,'execution','on','broad',tail,lab,self.package,self.evidence,self.use,'complete')

class ContractTests(unittest.TestCase):
    def test_frozen500_exact_disjoint_membership_and_separate104_scope(self):
        workspace=h.HERE.parents[3]
        rows=h.corpus(workspace);self.assertEqual(len(rows),500)
        shards=[h.shard_cases(workspace,i) for i in range(4)]
        self.assertEqual([len(s) for s in shards],[125]*4);self.assertEqual(len({c_['id'] for s in shards for c_,g in s}),500)
        self.assertEqual(h.broad_contract()['case_count'],104)
    def test_budget_does_not_execute_or_claim_completion(self):
        with patch.object(h.tail_process,'run',side_effect=AssertionError('must not run')):
            row=r.process(Path('/unused'),{'arms':{'on':{'source_sha':'x'}}},'on','joint',[],Path('/unused'),time.monotonic())
        self.assertEqual(row['status'],'not_run_budget');self.assertNotIn('process',row)
    def test_aggregate_missing_artifacts_explicitly_fails_without_engine(self):
        with tempfile.TemporaryDirectory() as temp:
            ws=Path(temp);package={'controller_sha':'c'*40,'source_sha':h.binding()['source_sha'],'run_id':'123','run_attempt':'1'}
            with patch.object(b,'use_package',return_value=(package,{})),patch.object(h,'corpus',return_value=[({'id':f'opening-{i:04d}'},{}) for i in range(500)]),patch.dict(os.environ,{'GITHUB_RUN_ID':'123'}):
                self.assertEqual(r.aggregate(ws,'a'*64),1)
            result=c.strict_json((ws/'p7f-aggregate-results/summary.json').read_bytes())
            self.assertFalse(result['full500_complete']);self.assertFalse(result['broad104_complete']);self.assertEqual(len(result['issues']),5)
    def test_broad_record_real_comparator_rejects_missing_cases_and_sample_drift(self):
        fixture=fixture_broad.BroadRecordsTests();manifest,rows=fixture.fixtures()
        self.assertTrue(fixture.compare(rows,rows,manifest)['passed'])
        with self.assertRaises(ValueError):fixture.compare(rows,rows[:1],manifest)
        altered=copy.deepcopy(rows);altered[0]['samples']={'State { changed }\nNone':1.}
        with self.assertRaises(ValueError):fixture.compare(rows,altered,manifest)
if __name__=='__main__':unittest.main()
