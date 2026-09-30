"""Instrument one frozen timeout case; never replace full500 or compare its timing."""
import argparse
import os
from pathlib import Path
import ci
import contract as c
import process_run
import diagnostic_process

CASE_ID='opening-0429'
PLAN_PATH='engine/benchmarks/turn_distribution/diagnostic0429-plan.json'
PLAN_SHA='2a6ec43ac6e821e1d710a94542f80f6ab04f37416406fb6a932b40f3d6de89ec'
PRIOR_RUN=36780443039
PRIOR_ARTIFACT_SHA='49af10d2907cbdc09ed539ce2852624013c2bc835f1f69062901323795367c41'
PRIOR_AUDIT_SHA='c30866f8fb15729b408b1111712550d835cc1c03e10b6454aff1828f69d77f56'

def fixed_case(workspace):
    manifest=c.corpus(workspace/'controller');case=manifest['cases'][429]
    c.require(case['id']==CASE_ID,'Frozen case identity differs')
    plan=c.safe_file(workspace/'controller',PLAN_PATH);c.require(c.sha(plan)==PLAN_SHA,'Original plan bytes changed')
    c.description(c.line(plan),case)
    return case,plan

def prepare(workspace):
    ci.prepare(workspace)
    result=workspace/'turn-distribution-results';path=result/'provenance.json'
    provenance=c.strict_json(path.read_bytes());case,plan=fixed_case(workspace)
    provenance.update(actual_requested_cases=1,diagnostic_case_ids=[CASE_ID],diagnostic_only=True,
        build_contract_scope='Inherited requested_cases/case_ids describe the frozen corpus build contract; diagnostic_case_ids is the executed scope',
        diagnostic_child_stats_configured=True,timing_comparable=False,
        purpose='Locate the original 60-second timeout; no timing comparison or full500 completion claim',
        timing_scope='Instrumented diagnostic child resources; API kernel fields are not comparative performance evidence',
        diagnostic_child_lab_environment={'LAB_ENGINE_STATS':'1'},
        original_plan_sha256=PLAN_SHA,prior_run=PRIOR_RUN,prior_artifact_sha256=PRIOR_ARTIFACT_SHA,prior_audit_sha256=PRIOR_AUDIT_SHA,
        limits={'description_seconds':60,'diagnostic_seconds':300,'rss_bytes_per_process':6*1024**3,'rss_poll_seconds':.01})
    ci.write(path,provenance)

def run(workspace):
    result=workspace/'turn-distribution-results'
    record={'schema':1,'case_id':CASE_ID,'status':'preparing','diagnostic_complete':False,
        'actual_requested_cases':1,'stats_environment_enabled':False,'timing_comparable':False,
        'source_sha':c.SOURCE_SHA,'corpus_sha256':c.CORPUS_SHA,'original_plan_sha256':PLAN_SHA,
        'prior_run':PRIOR_RUN,'prior_artifact_sha256':PRIOR_ARTIFACT_SHA,'prior_audit_sha256':PRIOR_AUDIT_SHA,
        'no_speed_comparison':True,'does_not_repair_full500_metrics':True,'official_full500_metrics':None,
        'description':{'status':'not_started'},'measurement':{'status':'not_started'}}
    path=result/'diagnostic-summary.json';c.require(not path.exists(),'Diagnostic already attempted')
    ci.write(path,record)
    try:
        receipt=c.strict_json((result/'build-receipt.json').read_bytes())
        c.require(receipt['status']=='success' and receipt['cached_regression_reused'] is False
            and receipt['source_sha']==c.SOURCE_SHA and receipt['corpus_sha256']==c.CORPUS_SHA
            and receipt['features']==list(c.CORE_FEATURES),'Fresh pinned build required')
        ci.verify_source(workspace/'source');case,original=fixed_case(workspace);env=ci.environment(workspace)
        c.require(not any(key.startswith('LAB_') for key in env),'Only diagnostic child may enable stats')
        binary=workspace/'target-distribution/release/lab-distribution-bench'
        c.require(c.sha(binary)==receipt['binary_sha256'] and Path(receipt['binary'])==binary,'Executable identity changed')
        record['binary_sha256']=receipt['binary_sha256']
        directory=result/'cases'/CASE_ID;directory.mkdir(parents=True,exist_ok=False)
        cpu=min(os.sched_getaffinity(0));scenario=c.safe_file(workspace/'controller',case['scenario'])
        for phase in ('description','measurement'):
            c.require(c.sha(scenario)==case['scenario_sha256'] and c.sha(binary)==receipt['binary_sha256'],'Inputs changed before child')
            c.require(c.sha(original)==PLAN_SHA,'Frozen plan changed before child')
            argv=[str(binary),str(scenario),'--joint-seed',str(case['joint_seed'])]
            stem=directory/phase;entry={'status':'running'};record[phase]=entry;ci.write(path,record)
            if phase=='description':
                argv+=['--describe'];entry['lab_environment']={}
                process=process_run.run(argv,workspace/'source/engine',env,stem,cpu=cpu)
            else:
                fresh=directory/'description.stdout'
                c.require(c.sha(fresh)==PLAN_SHA,'Regenerated plan changed before diagnostic')
                argv+=['--plan',str(fresh),'--sample-seeds',','.join(map(str,case['sample_seeds']))]
                entry['lab_environment']={'LAB_ENGINE_STATS':'1'}
                record['stats_environment_enabled']=True
                process=diagnostic_process.run(argv,workspace/'source/engine',dict(env,LAB_ENGINE_STATS='1'),stem,cpu=cpu)
            entry.update(process=process,status=process['status'])
            for channel in ('stdout','stderr'):
                raw=stem.with_suffix('.'+channel);entry[channel+'_sha256']=c.sha(raw);entry[channel+'_bytes']=raw.stat().st_size
            if phase=='measurement':
                text=stem.with_suffix('.stderr').read_text(encoding='utf-8',errors='replace')
                record['factored_stage_lines']=[line for line in text.splitlines() if line.startswith('lab-engine: factored stage ')]
                record['last_completed_factored_stage']=record['factored_stage_lines'][-1] if record['factored_stage_lines'] else None
                record['progress_limit']='Stats report completed factored stages only; no line cannot identify the exact active internal subphase'
            if process['status']!='ok':record['status']=process['status'];return 1
            value=c.line(stem.with_suffix('.stdout'))
            if phase=='description':
                c.description(value,case)
                c.require(stem.with_suffix('.stdout').read_bytes()==original.read_bytes(),'Regenerated plan does not exactly match original')
                record['original_plan_byte_equal']=True
            else:
                c.result(value,c.line(original),case)
                c.require(record['factored_stage_lines'],'Stats-enabled successful result lacked factored stage evidence')
                record['result']=value;record.update(status='ok',diagnostic_complete=True)
        return 0
    except Exception as error:
        record.update(status='validation_or_execution_error',error=f'{type(error).__name__}: {error}');return 1
    finally:ci.write(path,record)

def main():
    parser=argparse.ArgumentParser();parser.add_argument('stage',choices=('prepare','run'));parser.add_argument('--workspace',required=True,type=Path)
    args=parser.parse_args();result=globals()[args.stage](args.workspace.resolve())
    if result is not None:raise SystemExit(result)
if __name__=='__main__':main()
