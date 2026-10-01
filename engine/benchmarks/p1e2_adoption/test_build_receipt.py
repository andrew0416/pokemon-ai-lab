"""Exercise the real build -> verify_build boundary with mocked Cargo only."""
from contextlib import ExitStack
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch
import copy,json,tempfile,unittest
import adoption as a

class BuildFixture:
    """Emit protocol-shaped files; never run Git, Cargo, or an engine binary."""
    def __init__(self,workspace):
        self.workspace=workspace
        self.folder=workspace/a.s.RESULTS
        self.folder.mkdir()
        self.bound=a.s.binding()
        self.prior=a.s.prior_binding()
        self.calls=[]
        self.plans={arm:a.s.old_ci.command_plan(arm,self.prior)+[
            ('broad_build',a.broad_command(arm),None,None)] for arm in a.s.ARMS}

    def fingerprints(self,arm):
        root=self.workspace/('target-p1e2-'+arm)/'release/.fingerprint'
        names={'lab-engine':{'lib-lab_engine.json'}|({'test-lib-lab_engine.json'} if arm!='original' else set()),
            'lab-scenario':{'lib-lab_scenario.json','bin-lab-distribution-bench.json','test-bin-lab-distribution-bench.json',
                'test-integration-test-lazy_ko_damage.json','test-integration-test-factored.json',
                'test-integration-test-lazy_ko_damage_expanded.json'}}
        for package,files in names.items():
            folder=root/(package+'-synthetic');folder.mkdir(parents=True,exist_ok=True)
            for name in files:
                value={'features':json.dumps(a.s.p1.features(arm=='on') if package=='lab-engine' else []),
                    'rustflags':['-Ctarget-cpu=x86-64']}
                (folder/name).write_text(json.dumps(value),encoding='utf-8')

    def public_records(self,path):
        rows=[]
        for index,case in enumerate(a.s.old_compare.CASES):
            key='State { synthetic_case: '+str(index)+' }\nNone'
            rows.append({'schema':1,'case':case,'rolls':'Full' if case=='all-ko' else 'Extremes',
                'input':'synthetic-input-'+str(index),'input_hash':index,'components':1,
                'distribution':{key:1.0},'samples':{key:1.0}})
        path.write_text(''.join(json.dumps(row)+'\n' for row in rows),encoding='utf-8')

    def cargo(self,argv,*,cwd,env,stdout,stderr,timeout):
        arm=env['SYNTHETIC_CARGO_ARM']
        matches=[row for row in self.plans[arm] if row[1]==argv]
        if len(matches)!=1:raise AssertionError('Unexpected mocked Cargo command')
        label,_,tests,filtered=matches[0]
        assert cwd==a.s.source_root(self.workspace,arm)/'engine'
        assert stderr==a.subprocess.STDOUT and timeout==600
        self.calls.append((arm,label))
        self.fingerprints(arm)
        if label=='public_tests':self.public_records(Path(env['LAB_P1E_PUBLIC_RECORDS']))
        if label=='broad_build':
            binary=self.workspace/('target-p1e2-'+arm)/'release/deps/lazy_ko_damage_expanded-fixture'
            binary.parent.mkdir(parents=True,exist_ok=True);binary.write_bytes(b'synthetic binary; never executed')
            message={'reason':'compiler-artifact','target':{'name':a.s.BROAD_TARGET},
                'profile':{'test':True,'opt_level':'3'},'features':[],'executable':str(binary)}
            stdout.write((json.dumps(message)+'\n').encode())
        elif tests:
            lines=''.join('test '+name+' ... ok\n' for name in tests)
            lines+='\ntest result: ok. '+str(len(tests))+' passed; 0 failed; 0 ignored; 0 measured; '+str(filtered or 0)+' filtered out; finished in 0.00s\n'
            stdout.write(lines.encode())
        else:stdout.write(b'synthetic Cargo build success\n')
        return SimpleNamespace(returncode=0)

    def build(self):
        with ExitStack() as stack:
            stack.enter_context(patch.object(a.s,'verify_source',return_value={'synthetic':True}))
            stack.enter_context(patch.object(a.s,'verify_original',return_value={'synthetic':True}))
            stack.enter_context(patch.object(a.c,'corpus',return_value=None))
            stack.enter_context(patch.object(a.s,'environment',side_effect=lambda workspace,arm:{'SYNTHETIC_CARGO_ARM':arm}))
            stack.enter_context(patch.object(a.base_ci,'command',side_effect=AssertionError('Git/process execution forbidden in synthetic test')))
            stack.enter_context(patch.object(a.subprocess,'run',side_effect=self.cargo))
            a.build(self.workspace)
        assert self.calls==[(arm,row[0]) for arm in a.s.ARMS for row in self.plans[arm]]
        return self.receipt()

    def receipt(self):return json.loads((self.folder/'build-receipt.json').read_bytes())
    def save(self,value):a.base_ci.write(self.folder/'build-receipt.json',value)
    def verify(self):
        # No verification subroutine is mocked: read real synthetic files and all55 gates.
        with patch.object(a.base_ci,'command',side_effect=AssertionError('Git execution forbidden in synthetic verifier')):
            return a.verify_build(self.workspace,self.bound)

class BuildReceiptTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.fixture=BuildFixture(Path(self.temp.name))
        self.built=self.fixture.build()

    def test_build_then_verify_accepts_all_three_arms_and_broad_build_logs(self):
        self.assertEqual(self.fixture.verify(),self.built)
        self.assertEqual(self.built['inherited_named_test_executions'],55)
        rows=[row for row in self.built['commands'] if row['label']=='broad_build']
        self.assertEqual([row['log'] for row in rows],['original-broad-build.log','off-broad-build.log','on-broad-build.log'])
        self.assertTrue(all((self.fixture.folder/row['log']).is_file() for row in rows))

    def test_rejects_broad_log_rename_even_with_matching_bytes_and_hash(self):
        value=copy.deepcopy(self.built)
        row=next(row for row in value['commands'] if row['label']=='broad_build')
        good=self.fixture.folder/row['log'];bad=self.fixture.folder/'original-broad_build.log'
        bad.write_bytes(good.read_bytes());row['log']=bad.name
        self.fixture.save(value)
        with self.assertRaisesRegex(ValueError,'Fresh command/log changed'):self.fixture.verify()

    def test_rejects_broad_log_content_change(self):
        row=next(row for row in self.built['commands'] if row['label']=='broad_build')
        path=self.fixture.folder/row['log'];path.write_bytes(path.read_bytes()+b'altered\n')
        with self.assertRaisesRegex(ValueError,'Fresh command/log changed'):self.fixture.verify()

    def test_rejects_missing_broad_log(self):
        row=next(row for row in self.built['commands'] if row['label']=='broad_build')
        (self.fixture.folder/row['log']).unlink()
        with self.assertRaises(FileNotFoundError):self.fixture.verify()

    def test_rejects_missing_broad_command_receipt(self):
        value=copy.deepcopy(self.built)
        value['commands']=[row for row in value['commands'] if not(row['arm']=='on' and row['label']=='broad_build')]
        self.fixture.save(value)
        with self.assertRaisesRegex(ValueError,'Fresh command list incomplete'):self.fixture.verify()

    def test_rejects_non_test_broad_compiler_artifact_after_hash_refresh(self):
        value=copy.deepcopy(self.built)
        row=next(row for row in value['commands'] if row['label']=='broad_build')
        path=self.fixture.folder/row['log'];artifact=json.loads(path.read_bytes())
        artifact['profile']['test']=False;path.write_text(json.dumps(artifact)+'\n',encoding='utf-8')
        row['log_sha256']=a.c.sha(path);self.fixture.save(value)
        with self.assertRaisesRegex(ValueError,'Broad executable profile differs'):self.fixture.verify()

    def test_rejects_failed_inherited_test_even_with_refreshed_log_hash(self):
        value=copy.deepcopy(self.built)
        row=next(row for row in value['commands'] if row['label']=='public_tests')
        path=self.fixture.folder/row['log'];path.write_text(path.read_text().replace(' ... ok',' ... FAILED',1),encoding='utf-8')
        row['log_sha256']=a.c.sha(path);self.fixture.save(value)
        with self.assertRaisesRegex(ValueError,'Named tests missing or unexpected'):self.fixture.verify()

if __name__=='__main__':unittest.main()
