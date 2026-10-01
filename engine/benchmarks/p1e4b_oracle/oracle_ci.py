"""Bounded P1e4b original concrete oracle versus freshly rebuilt lazy candidate."""
import argparse,json,os,subprocess
from pathlib import Path
import oracle_shared as h
from oracle_shared import c,base_ci
import oracle_compare as compare
import tail_process
import process_run as describe_process

def prepare(workspace):
    folder=workspace/h.RESULTS;folder.mkdir(exist_ok=False);value={'status':'preparing','adoption_approved':False}
    try:
        bound=h.binding();sources=h.verify_sources(workspace,bound);case,plan=h.s.p1.fixed_case(workspace,'opening-0429')
        c.require(c.sha(plan)==bound['plan_sha256'],'Frozen plan changed')
        rustc=base_ci.command(['rustc','-Vv'],workspace);c.require(rustc.startswith('rustc 1.98.1 '),'Toolchain differs')
        value.update(status='prepared',sources=sources,source_binding_sha256=c.sha(h.HERE/'source-binding.json'),
          controller_sha=base_ci.command(['git','rev-parse','HEAD'],workspace/'controller'),rustc=rustc,
          lscpu=base_ci.command(['lscpu'],workspace),available_cpus=sorted(os.sched_getaffinity(0)),
          case=case,plan_sha256=c.sha(plan),features={arm:h.features(arm) for arm in h.ARMS},
          environment={k:v for k,v in h.environment(workspace,'oracle').items() if k.startswith(('CARGO_','RUST','RAYON','LAB_'))},
          timeout_seconds=h.TIMEOUTS,rss_limit_bytes=bound['rss_limit_bytes'],scratch_uploaded=False,
          scope='One original concrete-stream exact oracle versus candidate joint factored export. No timing ratio, full500 or adoption claim.')
    except Exception as error:value.update(status='failed',error=f'{type(error).__name__}: {error}');raise
    finally:base_ci.write(folder/'provenance.json',value)

def build(workspace):
    folder=workspace/h.RESULTS;path=folder/'build-receipt.json';c.require(not path.exists(),'Build already attempted')
    value={'status':'building','commands':[],'arms':{},'cached_regression_reused':False}
    try:
        bound=h.binding();h.verify_sources(workspace,bound)
        for arm in h.ARMS:
            target=h.target_root(workspace,arm);env=h.environment(workspace,arm)
            c.require(not any((target/'release/.fingerprint').glob('lab-*')),'Cached workspace build forbidden')
            for label,argv in h.commands(arm):
                log=folder/h.command_log_name(arm,label)
                row={'arm':arm,'label':label,'argv':argv,'log':log.name,'lab_environment':{k:v for k,v in env.items() if k.startswith('LAB_')}}
                value['commands'].append(row);base_ci.write(path,value)
                with log.open('xb') as stream:result=subprocess.run(argv,cwd=h.source_root(workspace,arm)/'engine',env=env,stdout=stream,stderr=subprocess.STDOUT,timeout=600)
                row.update(returncode=result.returncode,log_sha256=c.sha(log));c.require(result.returncode==0,'Fresh build/test failed')
                if label in h.SUITES:row['test_proof']=h.test_proof(label,log.read_text(encoding='utf-8'))
            binary=target/'release'/h.TARGETS[arm];c.require(binary.is_file() and not binary.is_symlink(),'Binary missing')
            proofs=h.fingerprints(workspace,arm)
            for proof in proofs:
                src=target/'release/.fingerprint'/proof['path'];dst=folder/'fingerprints'/arm/proof['path'];dst.parent.mkdir(parents=True,exist_ok=True)
                with dst.open('xb') as stream:stream.write(src.read_bytes())
            value['arms'][arm]={'binary':str(binary),'binary_sha256':c.sha(binary),'compiler_features':proofs,'features':h.features(arm)}
        value.update(status='success',oracle_source_sha=bound['oracle_source_sha'],candidate_source_sha=bound['candidate_source_sha'],
          source_binding_sha256=c.sha(h.HERE/'source-binding.json'),fresh_named_test_executions=17)
    except Exception as error:value.update(status='failed',error=f'{type(error).__name__}: {error}');raise
    finally:base_ci.write(path,value)

def verify_build(workspace,bound):
    folder=workspace/h.RESULTS;value=c.strict_json((folder/'build-receipt.json').read_bytes())
    c.require(value['status']=='success' and value['cached_regression_reused'] is False and value['oracle_source_sha']==bound['oracle_source_sha'] and value['candidate_source_sha']==bound['candidate_source_sha'] and value['source_binding_sha256']==c.sha(h.HERE/'source-binding.json') and value['fresh_named_test_executions']==17,'Fresh build receipt missing/mixed')
    expected=[(arm,label,argv) for arm in h.ARMS for label,argv in h.commands(arm)]
    c.require(set(value['arms'])==set(h.ARMS) and len(value['commands'])==len(expected),'Incomplete fresh build')
    for row,(arm,label,argv) in zip(value['commands'],expected):
        log=folder/h.command_log_name(arm,label)
        c.require((row['arm'],row['label'],row['argv'],row['returncode'])==(arm,label,argv,0),'Fresh command differs')
        c.require(row['lab_environment']=={} and row['log']==log.name and c.sha(log)==row['log_sha256'],'Fresh command/log differs')
        if label in h.SUITES:c.require(row['test_proof']==h.test_proof(label,log.read_text(encoding='utf-8')),'Fresh tests differ')
    for arm in h.ARMS:
        row=value['arms'][arm];binary=h.target_root(workspace,arm)/'release'/h.TARGETS[arm]
        c.require(row['binary']==str(binary) and c.sha(binary)==row['binary_sha256'],'Binary changed')
        c.require(row['features']==h.features(arm) and row['compiler_features']==h.fingerprints(workspace,arm),'Compiled features differ')
    return value

def inventory(root):return h.candidate_tail.inventory(root)
def verdict(arms):
    c.require(set(arms)==set(h.ARMS),'Both arms must be attempted')
    if any(row['status']=='invalid_complete_output' for row in arms.values()):return 'failed'
    return 'complete_reference' if all(row['status']=='complete' for row in arms.values()) else 'inconclusive'
def run_arms(execute):return {arm:execute(arm) for arm in h.ARMS}
def emit_verdict(value):
    message='P1e4b exact concrete reference: '+value['status']+'. Adoption is not authorized; no speed ratio or full500 claim.'
    if value['status']=='inconclusive':message+=' Original/candidate did not produce a complete verified export; opening-0429 agreement remains unproved.'
    print(('::notice::' if value['status']=='complete_reference' else '::warning::')+message)
    summary=os.environ.get('GITHUB_STEP_SUMMARY')
    if summary:
        with Path(summary).open('a',encoding='utf-8') as stream:stream.write('## P1e4b exact reference\n\n'+message+'\n')

def run(workspace):
    folder=workspace/h.RESULTS;path=folder/'oracle-summary.json';c.require(not path.exists(),'Oracle already attempted')
    value={'status':'preparing','arms':{},'adoption_approved':False,'full500_complete':False,'speed_ratio':None,'exact_reference_complete':False}
    try:
        bound=h.binding();h.verify_sources(workspace,bound);built=verify_build(workspace,bound)
        case,plan=h.s.p1.fixed_case(workspace,'opening-0429');c.require(c.sha(plan)==bound['plan_sha256'],'Plan changed')
        cpu=min(os.sched_getaffinity(0));scenario=c.safe_file(workspace/'controller',case['scenario'])
        value.update(oracle_source_sha=bound['oracle_source_sha'],candidate_source_sha=bound['candidate_source_sha'],
          source_binding_sha256=c.sha(h.HERE/'source-binding.json'),measurement_cpu=cpu,plan_sha256=c.sha(plan),
          case_id=case['id'],joint_seed=case['joint_seed'],fresh_named_test_executions=17,timeout_seconds=h.TIMEOUTS)
        for arm in h.ARMS:
            directory=folder/arm;directory.mkdir(exist_ok=False);stem=directory/'describe'
            binary=Path(built['arms'][arm]['binary']);argv=[str(binary),str(scenario),'--joint-seed',str(case['joint_seed']),'--describe']
            process=describe_process.run(argv,h.source_root(workspace,arm)/'engine',h.environment(workspace,arm),stem,cpu=cpu)
            row={'arm':arm,'status':'describing','binary_sha256':c.sha(binary),'description':{'argv':argv,'process':process},'lab_environment':{k:v for k,v in h.environment(workspace,arm).items() if k.startswith('LAB_')}}
            value['arms'][arm]=row
            for channel in ('stdout','stderr'):row['description'][channel+'_sha256']=c.sha(stem.with_suffix('.'+channel))
            base_ci.write(path,value)
            c.require(process['status']=='ok' and process['wall_seconds']<60 and stem.with_suffix('.stdout').read_bytes()==plan.read_bytes(),'Frozen description differs/incomplete')
        def execute(arm):
            row=value['arms'][arm];directory=folder/arm;output=directory/'export';stem=directory/'execution'
            binary=Path(built['arms'][arm]['binary']);c.require(c.sha(binary)==row['binary_sha256'],'Executable changed')
            argv=[str(binary),str(scenario),'--joint-seed',str(case['joint_seed']),'--plan',str(plan)]
            if arm=='oracle':
                scratch=workspace/'p1e4b-oracle-scratch';c.require(not scratch.exists(),'Scratch must be fresh')
                argv+=['--out',str(output),'--scratch',str(scratch)];row['scratch_path']=str(scratch);row['scratch_uploaded']=False
            else:argv+=['--output-dir',str(output),'--max-rows',str(compare.joint.MAX_ROWS),'--max-bytes',str(compare.joint.MAX_BYTES)]
            row.update(status='running',argv=argv,output_dir=str(output.relative_to(folder)));base_ci.write(path,value)
            try:
                process=tail_process.run(argv,h.source_root(workspace,arm)/'engine',h.environment(workspace,arm),stem,cpu=cpu,timeout_seconds=h.TIMEOUTS[arm],rss_limit_bytes=bound['rss_limit_bytes'])
                row.update(process=process,status='incomplete')
                for channel in ('stdout','stderr'):
                    raw=stem.with_suffix('.'+channel);row[channel+'_sha256']=c.sha(raw);row[channel+'_bytes']=raw.stat().st_size
                row['export_files']=inventory(output)
                if process['status']=='ok':
                    try:
                        actual=compare.read_oracle(output,plan) if arm=='oracle' else compare.joint.read_export(output,plan)
                        c.require(c.strict_json(stem.with_suffix('.stdout').read_bytes())==actual['manifest'],'Stdout/manifest differs')
                        row.update(status='complete',manifest=actual['manifest'],file_sha256=actual['file_sha256'],validated_rows=actual['rows'],raw_mass=actual['mass'])
                    except Exception as error:row.update(status='invalid_complete_output',error=f'{type(error).__name__}: {error}')
                else:row['incomplete_reason']=process['status']
            except Exception as error:
                row.update(status='incomplete',incomplete_reason='controller_or_execution_exception',error=f'{type(error).__name__}: {error}');row['export_files']=inventory(output)
            finally:base_ci.write(path,value)
            return row
        value['arms']=run_arms(execute);value['status']=verdict(value['arms'])
        if value['status']=='complete_reference':
            value['joint_agreement']=compare.compare_exports(folder/'oracle/export',folder/'candidate/export',plan)
            value['exact_reference_complete']=True
        else:value['joint_agreement']=None
        value['scope']='Single frozen Full-roll decision stopping at first complete mid-turn pause. Original concrete stage replay with external sorting versus candidate factored export; no timing comparison or global correctness/adoption claim.'
        return 1 if value['status']=='failed' else 0
    except Exception as error:value.update(status='failed',exact_reference_complete=False,error=f'{type(error).__name__}: {error}');return 1
    finally:base_ci.write(path,value);emit_verdict(value)

def main():
    parser=argparse.ArgumentParser();parser.add_argument('stage',choices=('prepare','build','run'));parser.add_argument('--workspace',required=True,type=Path)
    args=parser.parse_args();result=globals()[args.stage](args.workspace.resolve())
    if args.stage=='run':raise SystemExit(result)
if __name__=='__main__':main()
