"""All500 on one exact P1e candidate; immutable baseline input selection, no hybrid ledger."""
from pathlib import Path
import argparse,importlib.util,json,os,subprocess,sys,time
HERE=Path(__file__).resolve().parent
sys.path.insert(0,str(HERE.parent/'p1e2_adoption'))
import shared as s
from shared import c,base_ci
import bounded_process
spec=importlib.util.spec_from_file_location('p1e5_inherited_summary',HERE.parent/'turn_distribution/run_distribution.py')
inherited=importlib.util.module_from_spec(spec);spec.loader.exec_module(inherited)
SOURCE='56376838ffea91169321895f8e81abec2137dd46'
RESULTS='p1e5-full500-results';TARGET='target-p1e5-on';FEATURES=s.p1.features(True)
PLANS_SHA='2ebfa8608ab59c501790484c5c040e9ac0a5217e665dc92aff9827cf28107bd4'
GATE_SHA='a79f3eb740c97fcf4ef36aac3ad49741d748d82e2cc33c86fcf172358622bdb0'
WHOLE_SECONDS=1200;CASE_SECONDS=60;RSS_BYTES=6*1024**3

def contracts(workspace):
    bound=s.binding();c.require(bound['source_sha']==SOURCE,'Source must match audited P1e2 validation')
    c.require(c.sha(HERE/'reused-accuracy-audit.json')==GATE_SHA,'Reused correctness audit changed')
    gate=c.strict_json((HERE/'reused-accuracy-audit.json').read_bytes())
    c.require(gate['passed'] and gate['source_sha']==SOURCE and gate['fresh_named_test_executions']==61,'Exact-source correctness gate missing')
    c.require(all(row['passed'] and row['cases']==104 and row['distinct_input_count']==101 for row in gate['recomputed_comparisons'].values()),'Bounded prior correctness scope differs')
    manifest=c.corpus(workspace/'controller')
    c.require(c.sha(HERE/'original-plans.json')==PLANS_SHA,'Frozen original selection hash ledger changed')
    plans=c.strict_json((HERE/'original-plans.json').read_bytes())
    c.require(plans['baseline_run']==36780443039 and plans['baseline_source']==s.ORIGINAL and plans['corpus_sha256']==c.CORPUS_SHA,'Wrong original selection identity')
    c.require([row['id'] for row in plans['plans']]==[row['id'] for row in manifest['cases']] and len(plans['plans'])==500,'Incomplete plan hash set')
    return bound,manifest,plans

def environment(workspace):
    env=base_ci.environment(workspace);env['CARGO_TARGET_DIR']=str(workspace/TARGET);return env

def command_plan():
    return [('benchmark_tests',['cargo','test','--locked','--release','-p','lab-scenario','--bin','lab-distribution-bench',*s.p1.feature_args(True),'--','--test-threads=1'],list(c.TESTS)),
            ('benchmark_build',['cargo','build','--locked','--release','-p','lab-scenario','--bin','lab-distribution-bench',*s.p1.feature_args(True)],None)]

def command_log_name(label):
    c.require(label in ('benchmark_tests','benchmark_build'),'Unexpected build label');return label+'.log'

def fingerprints(workspace):
    root=workspace/TARGET/'release/.fingerprint';rows=[]
    c.require(not list(root.glob('lab-search-*')),'Unexpected search crate')
    for package,names in {'lab-engine':{'lib-lab_engine.json'},'lab-scenario':{'lib-lab_scenario.json','bin-lab-distribution-bench.json','test-bin-lab-distribution-bench.json'}}.items():
        found=set();expected=set(FEATURES) if package=='lab-engine' else set()
        for folder in sorted(root.glob(package+'-*')):
            for name in sorted(names):
                path=folder/name
                if not path.is_file():continue
                value=c.strict_json(path.read_bytes());features=value['features'];features=c.strict_json(features) if isinstance(features,str) else features
                c.require(isinstance(features,list) and len(features)==len(set(features)) and set(features)==expected,'Actual feature closure differs')
                c.require(value.get('rustflags')==['-Ctarget-cpu=x86-64'],'Actual generic target differs')
                rows.append({'path':str(path.relative_to(root)),'sha256':c.sha(path),'features':features,'content':value});found.add(name)
        c.require(found==names,'Missing actual compiled fingerprints '+package)
    return rows

def prepare(workspace):
    folder=workspace/RESULTS;folder.mkdir(exist_ok=False);value={'status':'preparing','adoption_approved':False}
    try:
        bound,manifest,plans=contracts(workspace);source=s.verify_source(workspace/'source',bound);env=environment(workspace)
        rustc=base_ci.command(['rustc','-Vv'],workspace);c.require(rustc.startswith('rustc 1.98.1 '),'Toolchain differs')
        value.update(status='prepared',controller_sha=base_ci.command(['git','rev-parse','HEAD'],workspace/'controller'),source=source,source_sha=SOURCE,
          inherited_runtime='P1e a10 runtime unchanged; test-only validation source563',benchmark_sha256=s.p1.BENCHMARK_SHA,
          corpus_sha256=c.CORPUS_SHA,original_plans_sha256=PLANS_SHA,case_ids=[row['id'] for row in manifest['cases']],features=FEATURES,
          reused_accuracy_audit_sha256=GATE_SHA,reused_accuracy_run=36835592119,reused_named_test_executions=61,fresh_benchmark_tests=7,
          rustc=rustc,lscpu=base_ci.command(['lscpu'],workspace),available_cpus=sorted(os.sched_getaffinity(0)),
          environment={key:env[key] for key in ('RUSTFLAGS','CARGO_INCREMENTAL','CARGO_PROFILE_RELEASE_OPT_LEVEL','CARGO_PROFILE_RELEASE_DEBUG','CARGO_PROFILE_RELEASE_LTO','CARGO_PROFILE_RELEASE_CODEGEN_UNITS','RAYON_NUM_THREADS')},
          lab_environment_absent=not any(key.startswith('LAB_') for key in env),limits={'per_child_seconds':60,'rss_bytes':RSS_BYTES,'whole_measurement_seconds':WHOLE_SECONDS,'one_cpu':True},
          reference_scope='Same candidate exact Full factored distribution; not original0429 equivalence or paper reproduction.',original0429_oracle_complete=False,
          mixed_source_ledger=False,paired_speed_comparison=False,repairs_original_full500=False)
    except Exception as error:value.update(status='failed',error=f'{type(error).__name__}: {error}');raise
    finally:base_ci.write(folder/'provenance.json',value)

def build(workspace):
    folder=workspace/RESULTS;path=folder/'build-receipt.json';c.require(not path.exists(),'Build already attempted')
    value={'status':'building','commands':[],'cached_regression_reused':False}
    try:
        bound,_,_=contracts(workspace);s.verify_source(workspace/'source',bound);env=environment(workspace)
        c.require(not any((workspace/TARGET/'release/.fingerprint').glob('lab-*')),'Workspace cache cannot skip fresh tests')
        for label,argv,tests in command_plan():
            log=folder/command_log_name(label);row={'label':label,'argv':argv,'log':log.name,'lab_environment':{k:v for k,v in env.items() if k.startswith('LAB_')}}
            value['commands'].append(row);base_ci.write(path,value)
            with log.open('xb') as stream:result=subprocess.run(argv,cwd=workspace/'source/engine',env=env,stdout=stream,stderr=subprocess.STDOUT,timeout=600)
            row.update(returncode=result.returncode,log_sha256=c.sha(log));c.require(result.returncode==0,'Fresh benchmark test/build failed')
            if tests:row['test_proof']=s.old_ci.named_test_proof(log.read_text(),tests,0)
        binary=workspace/TARGET/'release/lab-distribution-bench';c.require(binary.is_file() and not binary.is_symlink(),'Missing binary')
        proofs=fingerprints(workspace)
        for proof in proofs:
            src=workspace/TARGET/'release/.fingerprint'/proof['path'];dst=folder/'fingerprints'/proof['path'];dst.parent.mkdir(parents=True,exist_ok=True)
            with dst.open('xb') as stream:stream.write(src.read_bytes())
        value.update(status='success',source_sha=SOURCE,features=FEATURES,corpus_sha256=c.CORPUS_SHA,reused_accuracy_audit_sha256=GATE_SHA,
          original_plans_sha256=PLANS_SHA,binary=str(binary),binary_sha256=c.sha(binary),compiler_features=proofs,fresh_named_test_executions=7)
    except Exception as error:value.update(status='failed',error=f'{type(error).__name__}: {error}');raise
    finally:base_ci.write(path,value)

def verify_build(workspace):
    folder=workspace/RESULTS;value=c.strict_json((folder/'build-receipt.json').read_bytes())
    c.require(value['status']=='success' and value['source_sha']==SOURCE and value['cached_regression_reused'] is False,'Fresh candidate build required')
    c.require(value['features']==FEATURES and value['corpus_sha256']==c.CORPUS_SHA and value['reused_accuracy_audit_sha256']==GATE_SHA and value['original_plans_sha256']==PLANS_SHA,'Frozen build identity changed')
    expected=command_plan();c.require(len(value['commands'])==len(expected),'Incomplete build commands');count=0
    for row,(label,argv,tests) in zip(value['commands'],expected):
        log=folder/command_log_name(label)
        c.require((row['label'],row['argv'],row['returncode'],row['lab_environment'])==(label,argv,0,{}),'Build command changed')
        c.require(row['log']==log.name and c.sha(log)==row['log_sha256'],'Build log identity/hash changed')
        if tests:
            proof=s.old_ci.named_test_proof(log.read_text(),tests,0);c.require(proof==row['test_proof'],'Fresh test proof changed');count+=proof['passed']
    c.require(count==value['fresh_named_test_executions']==7,'Missing fresh tests')
    c.require(fingerprints(workspace)==value['compiler_features'],'Actual compiler features changed')
    binary=workspace/TARGET/'release/lab-distribution-bench'
    c.require(value['binary']==str(binary) and binary.is_file() and not binary.is_symlink() and c.sha(binary)==value['binary_sha256'],'Binary changed')
    return value

def summary(records):
    c.require(len(records)==500 and all(row.get('source_sha')==SOURCE for row in records),'Mixed-source or incomplete full500 ledger forbidden')
    result=inherited.summary(records,500)
    result.update(source_sha=SOURCE,features=FEATURES,candidate_full500_complete=result['complete'],mixed_source_ledger=False,
      metric_reference='P1e candidate self-reference on all500 fixed cases; no cross-VM paired speedup',adoption_approved=False,
      original0429_oracle_complete=False,repairs_original_full500=False,reused_accuracy_audit_sha256=GATE_SHA)
    return result

def run(workspace):
    folder=workspace/RESULTS;bound,manifest,plans=contracts(workspace);s.verify_source(workspace/'source',bound)
    receipt=verify_build(workspace);env=environment(workspace);binary=Path(receipt['binary']);cpu=min(os.sched_getaffinity(0))
    out=folder/'cases';out.mkdir(exist_ok=False);started=time.monotonic();deadline=started+WHOLE_SECONDS
    records=[{'id':case['id'],'source_sha':SOURCE,'description':{'status':'pending'},'measurement':{'status':'pending'}} for case in manifest['cases']]
    def save():
        base_ci.write(folder/'records.json',{'schema':1,'source_sha':SOURCE,'features':FEATURES,'binary_sha256':receipt['binary_sha256'],'corpus_sha256':c.CORPUS_SHA,'original_plans_sha256':PLANS_SHA,'requested_cases':500,'cases':records})
        value=summary(records);value.update(measurement_stage_wall_seconds=time.monotonic()-started,whole_measurement_limit_seconds=WHOLE_SECONDS,whole_limit_enforcement='Remaining budget is passed to each bounded child; Python validation and artifact bookkeeping may finish after deadline.')
        base_ci.write(folder/'summary.json',value)
    save()
    for phase in ('description','measurement'):
        if phase=='measurement' and any(row['description']['status']!='ok' for row in records):
            for row in records:row['measurement']={'status':'not_run_description_failure'}
            save();return 1
        for case,row,golden in zip(manifest['cases'],records,plans['plans']):
            remaining=deadline-time.monotonic()
            if remaining<=0:
                for pending in records:
                    if pending[phase]['status']=='pending':pending[phase]={'status':'not_run_whole_budget'}
                save();return 1
            directory=out/case['id'];directory.mkdir(exist_ok=True);stem=directory/phase
            scenario=c.safe_file(workspace/'controller',case['scenario']);c.require(c.sha(scenario)==case['scenario_sha256'] and c.sha(binary)==receipt['binary_sha256'],'Frozen input/binary changed')
            argv=[str(binary),str(scenario),'--joint-seed',str(case['joint_seed'])]
            if phase=='description':argv+=['--describe']
            else:
                plan=directory/'description.stdout';c.require(c.sha(plan)==golden['sha256']==row['description']['stdout_sha256'],'Original frozen plan changed')
                argv+=['--plan',str(plan),'--sample-seeds',','.join(map(str,case['sample_seeds']))]
            entry={'status':'running','argv':argv,'lab_environment':{k:v for k,v in env.items() if k.startswith('LAB_')}};row[phase]=entry;save()
            try:
                remaining=deadline-time.monotonic();c.require(remaining>0,'Whole measurement budget exhausted before child')
                process=bounded_process.run(argv,workspace/'source/engine',env,stem,cpu=cpu,timeout_seconds=min(CASE_SECONDS,remaining))
                entry.update(process=process,status=process['status'])
                for channel in ('stdout','stderr'):
                    path=stem.with_suffix('.'+channel);entry[channel+'_sha256']=c.sha(path);entry[channel+'_bytes']=path.stat().st_size
                if process['status']=='ok':
                    value=c.line(stem.with_suffix('.stdout'))
                    if phase=='description':
                        c.description(value,case);c.require(entry['stdout_sha256']==golden['sha256'] and entry['stdout_bytes']==golden['bytes'],'Candidate selected different original opening bytes')
                    else:c.result(value,c.line(directory/'description.stdout'),case);row['value']=value
            except Exception as error:entry.update(status='validation_or_execution_error',error=f'{type(error).__name__}: {error}')
            save()
    return 0 if summary(records)['complete'] else 1

def main():
    parser=argparse.ArgumentParser();parser.add_argument('stage',choices=('prepare','build','run'));parser.add_argument('--workspace',type=Path,required=True);args=parser.parse_args()
    result=globals()[args.stage](args.workspace.resolve())
    if args.stage=='run':raise SystemExit(result)
if __name__=='__main__':main()
