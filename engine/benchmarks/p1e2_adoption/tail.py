"""Separate bounded exact-reference attempt; never infer accuracy from a timeout."""
import argparse, json, os, subprocess
from pathlib import Path
import shared as s
from shared import c,base_ci
import compare_joint as joint
import tail_process
import process_run as describe_process

RESULTS='p1e2-tail-results'
ARMS=('original','on')
TARGET='lab-joint-export'
SOURCE='engine/scenario/src/bin/lab-joint-export.rs'
TESTS=[
 'tests::joint_export_overlap_matches_independent_flat_expansion',
 'tests::joint_export_preserves_hp_correlation_and_last_reserve',
 'tests::joint_export_retains_hidden_non_hp_fields_and_dictionary_order',
 'tests::joint_export_rejects_bad_factors_and_mass',
 'tests::joint_export_row_encoding_and_limits_are_explicit',
 'tests::joint_export_plan_comparison_requires_exact_bytes',
]
TIMEOUTS={'original':1200,'on':300}
CONTRACT={'enabled':True,'case_id':'opening-0429','arms':list(ARMS),'target':TARGET,'source_path':SOURCE,
 'tests':TESTS,'timeout_seconds':TIMEOUTS,'rss_limit_bytes':6*1024**3,'max_rows':joint.MAX_ROWS,
 'max_bytes_including_manifest':joint.MAX_BYTES,'job_timeout_minutes':35,'run_candidate_after_original_incomplete':True,
 'incomplete_reference_verdict':'inconclusive','adoption_if_incomplete':False,'accuracy_only':True}

def environment(workspace,arm):
    c.require(arm in ARMS,'Bad tail arm')
    env=s.environment(workspace,arm);env['CARGO_TARGET_DIR']=str(workspace/('target-p1e2-tail-'+arm));return env

def commands(arm):
    return [('exporter_tests',['cargo','test','--locked','--release','-p','lab-scenario','--bin',TARGET,*s.p1.feature_args(arm=='on'),'--','--test-threads=1','--show-output']),
            ('exporter_build',['cargo','build','--locked','--release','-p','lab-scenario','--bin',TARGET,*s.p1.feature_args(arm=='on')])]

def fingerprints(workspace,arm):
    root=workspace/('target-p1e2-tail-'+arm)/'release/.fingerprint';rows=[]
    c.require(not list(root.glob('lab-search-*')),'Unexpected search crate')
    for package,names in {'lab-engine':{'lib-lab_engine.json'},'lab-scenario':{'lib-lab_scenario.json','bin-'+TARGET+'.json','test-bin-'+TARGET+'.json'}}.items():
        seen=set();expected=set(s.p1.features(arm=='on')) if package=='lab-engine' else set()
        for folder in sorted(root.glob(package+'-*')):
            for name in sorted(names):
                path=folder/name
                if not path.is_file():continue
                value=c.strict_json(path.read_bytes());features=value['features']
                if isinstance(features,str):features=c.strict_json(features)
                c.require(isinstance(features,list) and len(features)==len(set(features)) and set(features)==expected,'Tail actual features differ')
                c.require(value.get('rustflags')==['-Ctarget-cpu=x86-64'],'Tail generic target differs')
                seen.add(name);rows.append({'path':str(path.relative_to(root)),'sha256':c.sha(path),'features':features,'content':value})
        c.require(seen==names,'Missing tail fingerprints '+arm+' '+package)
    return rows

def bound_contract(bound):
    c.require(bound['tail_contract']==CONTRACT,'Tail bounds/identity differ')
    c.require(SOURCE in bound['test_only_file_sha256'],'Unpinned exporter source')
    c.require(bound['joint_comparator_sha256']==c.sha(s.HERE/'compare_joint.py'),'Joint comparator changed')

def prepare(workspace):
    folder=workspace/RESULTS;folder.mkdir(exist_ok=False)
    value={'status':'preparing','accuracy_only':True,'adoption_approved':False}
    try:
        bound=s.binding();bound_contract(bound)
        value['candidate_source']=s.verify_source(workspace/'source',bound)
        value['original_source']=s.prepare_original(workspace,bound)
        case,plan=s.p1.fixed_case(workspace,'opening-0429')
        rustc=base_ci.command(['rustc','-Vv'],workspace);c.require(rustc.startswith('rustc 1.98.1 '),'Toolchain differs')
        value.update(status='prepared',source_sha=bound['source_sha'],source_binding_sha256=c.sha(s.HERE/'source-binding.json'),
          controller_sha=base_ci.command(['git','rev-parse','HEAD'],workspace/'controller'),rustc=rustc,
          lscpu=base_ci.command(['lscpu'],workspace),available_cpus=sorted(os.sched_getaffinity(0)),
          contract=CONTRACT,case=case,plan_sha256=c.sha(plan),features={arm:s.p1.features(arm=='on') for arm in ARMS},
          environment={k:v for k,v in environment(workspace,'original').items() if k.startswith(('CARGO_','RUST','RAYON','LAB_'))},
          scope='Single-case bounded exact reference feasibility and full joint distribution agreement; no paired speed or full500 claim.')
    except Exception as error:
        value.update(status='failed',error=f'{type(error).__name__}: {error}');raise
    finally:base_ci.write(folder/'provenance.json',value)

def build(workspace):
    folder=workspace/RESULTS;path=folder/'build-receipt.json'
    c.require(not path.exists(),'Tail build already attempted')
    value={'status':'building','commands':[],'arms':{},'cached_regression_reused':False}
    try:
        bound=s.binding();bound_contract(bound)
        s.verify_source(workspace/'source',bound);s.verify_original(workspace/'original',bound)
        for arm in ARMS:
            target=workspace/('target-p1e2-tail-'+arm);env=environment(workspace,arm)
            c.require(not any((target/'release/.fingerprint').glob('lab-*')),'Cached workspace artifacts forbidden')
            for label,argv in commands(arm):
                log=folder/(arm+'-'+label+'.log');row={'arm':arm,'label':label,'argv':argv,'log':log.name,'lab_environment':{k:v for k,v in env.items() if k.startswith('LAB_')}}
                value['commands'].append(row);base_ci.write(path,value)
                with log.open('xb') as stream:
                    result=subprocess.run(argv,cwd=s.source_root(workspace,arm)/'engine',env=env,stdout=stream,stderr=subprocess.STDOUT,timeout=600)
                row.update(returncode=result.returncode,log_sha256=c.sha(log))
                c.require(result.returncode==0,'Exporter fresh build/test failed')
                if label=='exporter_tests':row['test_proof']=s.old_ci.named_test_proof(log.read_text(encoding='utf-8'),TESTS,0)
            binary=target/'release'/TARGET;c.require(binary.is_file() and not binary.is_symlink(),'Exporter binary missing')
            proofs=fingerprints(workspace,arm)
            for proof in proofs:
                src=target/'release/.fingerprint'/proof['path'];dst=folder/'fingerprints'/arm/proof['path'];dst.parent.mkdir(parents=True,exist_ok=True)
                with dst.open('xb') as out:out.write(src.read_bytes())
            value['arms'][arm]={'binary':str(binary),'binary_sha256':c.sha(binary),'compiler_features':proofs,'features':s.p1.features(arm=='on')}
        value.update(status='success',source_sha=bound['source_sha'],source_binding_sha256=c.sha(s.HERE/'source-binding.json'),fresh_named_test_executions=12)
    except Exception as error:
        value.update(status='failed',error=f'{type(error).__name__}: {error}');raise
    finally:base_ci.write(path,value)

def verify_build(workspace,bound):
    folder=workspace/RESULTS;value=c.strict_json((folder/'build-receipt.json').read_bytes())
    c.require(value['status']=='success' and value['cached_regression_reused'] is False and value['source_sha']==bound['source_sha']
      and value['source_binding_sha256']==c.sha(s.HERE/'source-binding.json') and value['fresh_named_test_executions']==12,'Fresh tail build missing')
    expected=[(arm,label,argv) for arm in ARMS for label,argv in commands(arm)]
    c.require(set(value['arms'])==set(ARMS) and len(value['commands'])==len(expected),'Tail build incomplete')
    for row,(arm,label,argv) in zip(value['commands'],expected):
        log=folder/(arm+'-'+label+'.log')
        c.require((row['arm'],row['label'],row['argv'],row['returncode'])==(arm,label,argv,0),'Tail command identity differs')
        c.require(row['lab_environment']=={} and row['log']==log.name and c.sha(log)==row['log_sha256'],'Tail build log/environment differs')
        if label=='exporter_tests':c.require(row['test_proof']==s.old_ci.named_test_proof(log.read_text(encoding='utf-8'),TESTS,0),'Tail fresh tests differ')
    for arm in ARMS:
        row=value['arms'][arm];binary=workspace/('target-p1e2-tail-'+arm)/'release'/TARGET
        c.require(row['binary']==str(binary) and c.sha(binary)==row['binary_sha256'],'Tail binary changed')
        c.require(row['features']==s.p1.features(arm=='on') and row['compiler_features']==fingerprints(workspace,arm),'Tail compiled features differ')
    return value

def inventory(root):
    if not root.exists():return []
    c.require(root.is_dir() and not root.is_symlink(),'Unsafe export directory')
    return [{'name':str(path.relative_to(root)),'bytes':path.stat().st_size,'sha256':joint.file_sha(path)}
            for path in sorted(root.rglob('*')) if path.is_file() and not path.is_symlink()]

def result_verdict(arms):
    c.require(set(arms)==set(ARMS),'Both tail arms must be attempted')
    if any(row['status']=='invalid_complete_output' for row in arms.values()):return 'failed'
    if all(row['status']=='complete' for row in arms.values()):return 'complete_reference'
    return 'inconclusive'

def run_arms(execute):
    # A bounded failed/incomplete reference must not suppress the candidate attempt.
    return {arm:execute(arm) for arm in ARMS}

def emit_verdict(value):
    status=value['status']
    message='P1e2 exact reference: '+status+'. Adoption is not authorized. See both accuracy and exact-reference artifacts.'
    if status=='inconclusive':message+=' Original/candidate reference incomplete; accuracy remains unproved for opening-0429.'
    print(('::warning::' if status in ('inconclusive','failed') else '::notice::')+message)
    summary=os.environ.get('GITHUB_STEP_SUMMARY')
    if summary:
        with Path(summary).open('a',encoding='utf-8') as stream:stream.write('## P1e2 exact-reference verdict\n\n'+message+'\n')

def run(workspace):
    folder=workspace/RESULTS;path=folder/'tail-summary.json'
    c.require(not path.exists(),'Tail already attempted')
    value={'status':'preparing','arms':{},'adoption_approved':False,'accuracy_only':True,
           'full500_complete':False,'official_full500_metrics':None,'kernel_speed_ratio':None,'completed_pair_speedup':None}
    try:
        bound=s.binding();bound_contract(bound);s.verify_source(workspace/'source',bound);s.verify_original(workspace/'original',bound)
        built=verify_build(workspace,bound);case,plan=s.p1.fixed_case(workspace,'opening-0429');cpu=min(os.sched_getaffinity(0))
        value.update(source_sha=bound['source_sha'],source_binding_sha256=c.sha(s.HERE/'source-binding.json'),measurement_cpu=cpu,contract=CONTRACT,
                     plan_sha256=c.sha(plan),case_id=case['id'],joint_seed=case['joint_seed'],fresh_named_test_executions=12)
        scenario=c.safe_file(workspace/'controller',case['scenario'])
        # Cheap description checks for both arms precede any expensive reference attempt.
        for arm in ARMS:
            directory=folder/arm;directory.mkdir(exist_ok=False);stem=directory/'describe'
            binary=Path(built['arms'][arm]['binary']);env=environment(workspace,arm)
            argv=[str(binary),str(scenario),'--joint-seed',str(case['joint_seed']),'--describe']
            process=describe_process.run(argv,s.source_root(workspace,arm)/'engine',env,stem,cpu=cpu)
            row={'arm':arm,'status':'describing','binary_sha256':c.sha(binary),'description':{'argv':argv,'process':process},'lab_environment':{k:v for k,v in env.items() if k.startswith('LAB_')}}
            value['arms'][arm]=row
            for channel in ('stdout','stderr'):row['description'][channel+'_sha256']=c.sha(stem.with_suffix('.'+channel))
            base_ci.write(path,value)
            c.require(process['status']=='ok' and stem.with_suffix('.stdout').read_bytes()==plan.read_bytes(),'Frozen tail selection bytes differ')
        def execute(arm):
            row=value['arms'][arm];directory=folder/arm;output=directory/'export';stem=directory/'execution'
            binary=Path(built['arms'][arm]['binary']);c.require(c.sha(binary)==row['binary_sha256'],'Tail executable changed')
            argv=[str(binary),str(scenario),'--joint-seed',str(case['joint_seed']),'--plan',str(plan),'--output-dir',str(output),
                  '--max-rows',str(joint.MAX_ROWS),'--max-bytes',str(joint.MAX_BYTES)]
            row.update(status='running',argv=argv,output_dir=str(output.relative_to(folder)));base_ci.write(path,value)
            try:
                process=tail_process.run(argv,s.source_root(workspace,arm)/'engine',environment(workspace,arm),stem,cpu=cpu,timeout_seconds=TIMEOUTS[arm])
                row.update(process=process,status='incomplete')
                for channel in ('stdout','stderr'):
                    raw=stem.with_suffix('.'+channel);row[channel+'_sha256']=c.sha(raw);row[channel+'_bytes']=raw.stat().st_size
                row['export_files']=inventory(output)
                if process['status']=='ok':
                    try:
                        actual=joint.read_export(output,plan)
                        c.require(c.strict_json(stem.with_suffix('.stdout').read_bytes())==actual['manifest'],'Exporter stdout/manifest differs')
                        row.update(status='complete',manifest=actual['manifest'],file_sha256=actual['file_sha256'],validated_rows=actual['rows'],raw_mass=actual['mass'])
                    except Exception as error:row.update(status='invalid_complete_output',error=f'{type(error).__name__}: {error}')
                else:row['incomplete_reason']=process['status']
            except Exception as error:
                row.update(status='incomplete',incomplete_reason='controller_or_execution_exception',error=f'{type(error).__name__}: {error}')
                row['export_files']=inventory(output)
            finally:base_ci.write(path,value)
            return row
        value['arms']=run_arms(execute)
        value['status']=result_verdict(value['arms'])
        if value['status']=='complete_reference':
            value['joint_agreement']=joint.compare_exports(folder/'original/export',folder/'on/export',plan)
            value['exact_reference_complete']=True
        else:
            value['exact_reference_complete']=False
            value['joint_agreement']=None
        value['scope']='One fixed Full-roll joint-State reference, original and candidate; incomplete reference prevents adoption. Timing is diagnostic only, not a speed ratio or full500 result.'
        return 1 if value['status']=='failed' else 0
    except Exception as error:
        value.update(status='failed',exact_reference_complete=False,error=f'{type(error).__name__}: {error}');return 1
    finally:
        base_ci.write(path,value)
        emit_verdict(value)

def main():
    parser=argparse.ArgumentParser();parser.add_argument('stage',choices=('prepare','build','run'));parser.add_argument('--workspace',required=True,type=Path)
    args=parser.parse_args();result=globals()[args.stage](args.workspace.resolve())
    if args.stage=='run':raise SystemExit(result)
if __name__=='__main__':main()