"""Check one bounded OFF/ON control before the isolated instrumented timeout case."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import ci
from common import c,base_ci,binding,features,fixed_case,semantic_result,ENVIRONMENT,PLAN_SHA
import process_run
import diagnostic_process
import observer_trace as trace

def execute(workspace,receipt,case_id,arm,directory,cpu,diagnostic=False):
    case,original=fixed_case(workspace,case_id);env=ci.environment(workspace,arm)
    c.require(not any(k.startswith('LAB_') for k in env),'Clean child environment required')
    proof=receipt['arms'][arm];binary=workspace/('target-p17-'+arm)/'release/lab-distribution-bench'
    c.require(str(binary)==proof['binary'] and c.sha(binary)==proof['binary_sha256'],'Arm binary changed')
    scenario=c.safe_file(workspace/'controller',case['scenario']);directory.mkdir(parents=True,exist_ok=False)
    entry={'case_id':case_id,'arm':arm,'binary_sha256':proof['binary_sha256'],'timing_comparable':False}
    # Save after each phase so raw and structured evidence survive a later timeout/error.
    path=directory/'receipt.json'
    try:
        for phase in ('description','measurement'):
            c.require(c.sha(binary)==proof['binary_sha256'] and c.sha(scenario)==case['scenario_sha256'],'Binary/scenario changed before child')
            c.require(c.sha(original)==PLAN_SHA[case_id],'Original plan changed before child')
            argv=[str(binary),str(scenario),'--joint-seed',str(case['joint_seed'])]
            stem=directory/phase;row={'status':'running','lab_environment':{}};entry[phase]=row;base_ci.write(path,entry)
            if phase=='description':argv+=['--describe'];runner=process_run;child_env=env
            else:
                plan=directory/'description.stdout';c.require(c.sha(plan)==PLAN_SHA[case_id],'Fresh description differs from original')
                argv+=['--plan',str(plan),'--sample-seeds',','.join(map(str,case['sample_seeds']))]
                runner=diagnostic_process if diagnostic else process_run
                child_env=dict(env,**ENVIRONMENT) if arm=='on' else env
                row['lab_environment']=ENVIRONMENT if arm=='on' else {}
            process=runner.run(argv,workspace/'source/engine',child_env,stem,cpu=cpu)
            row.update(status=process['status'],process=process)
            for channel in ('stdout','stderr'):
                raw=stem.with_suffix('.'+channel);row[channel+'_sha256']=c.sha(raw);row[channel+'_bytes']=raw.stat().st_size
            stderr=stem.with_suffix('.stderr').read_bytes()
            if phase=='measurement' and arm=='on':
                row['observer_trace']=trace.parse(stderr,complete=process['status']=='ok',required=True)
            else:c.require(b'P17_FRONTIER ' not in stderr,'Observer leaked into OFF/description child')
            if process['status']!='ok':entry['status']=process['status'];return entry
            value=c.line(stem.with_suffix('.stdout'))
            if phase=='description':
                c.description(value,case);c.require(stem.with_suffix('.stdout').read_bytes()==original.read_bytes(),'Fresh description bytes changed')
            else:
                entry['semantic_result']=semantic_result(value,c.line(original),case)
                entry['semantic_sha256']=hashlib.sha256(json.dumps(entry['semantic_result'],sort_keys=True,allow_nan=False,separators=(',',':')).encode()).hexdigest()
                entry['result']=value;entry['status']='ok'
        return entry
    except Exception as error:
        entry.update(status='validation_or_execution_error',error=f'{type(error).__name__}: {error}');return entry
    finally:base_ci.write(path,entry)

def run(workspace):
    folder=workspace/'p17-diagnostic-results';path=folder/'diagnostic-summary.json'
    c.require(not path.exists(),'Diagnosis already attempted')
    record={'schema':1,'status':'preparing','diagnostic_only':True,'timing_comparable':False,
        'official_full500_metrics':None,'repairs_full500_metrics':False,
        'control_equality_scope':'Serialized benchmark measurement result excluding only reference.kernel_ns, reference.metric_prepare_ns, samples[].kernel_ns and samples[].metric_ns; not an exhaustive emitted component/state list',
        'control':{},'diagnostic':{'status':'not_started'}}
    base_ci.write(path,record)
    try:
        bound=binding();ci.verify_source(workspace/'source',bound)
        receipt=c.strict_json((folder/'build-receipt.json').read_bytes())
        c.require(receipt['status']=='success' and receipt['cached_regression_reused'] is False
            and receipt['source_sha']==bound['source_sha'] and receipt['corpus_sha256']==c.CORPUS_SHA,'Fresh bound build required')
        c.require(set(receipt['arms'])=={'off','on'},'Missing OFF/ON arm')
        for arm in ('off','on'):
            c.require(receipt['arms'][arm]['features']==features(arm=='on'),'Unexpected compiled arm features')
            c.require(ci.fingerprints(workspace,arm,arm=='on')==receipt['arms'][arm]['compiler_features'],'Actual compiler fingerprints changed after build')
        record.update(source_sha=bound['source_sha'],corpus_sha256=c.CORPUS_SHA,observer_environment=ENVIRONMENT)
        cpu=min(os.sched_getaffinity(0))
        for arm in ('off','on'):
            entry=execute(workspace,receipt,'opening-0000',arm,folder/'control'/arm,cpu)
            record['control'][arm]=entry;base_ci.write(path,record)
            c.require(entry['status']=='ok','Bounded control failed: '+arm)
        left,right=record['control']['off'],record['control']['on']
        c.require(left['semantic_result']==right['semantic_result'],'Observer changed bounded control result beyond explicit timing fields')
        record['control_semantic_equal']=True;record['control_semantic_sha256']=left['semantic_sha256']
        entry=execute(workspace,receipt,'opening-0429','on',folder/'diagnostic'/'opening-0429',cpu,diagnostic=True)
        record['diagnostic']=entry;record['status']=entry['status'];record['diagnostic_complete']=entry['status']=='ok'
        return 0 if entry['status']=='ok' else 1
    except Exception as error:
        record.update(status='validation_or_execution_error',error=f'{type(error).__name__}: {error}');return 1
    finally:base_ci.write(path,record)

def main():
    parser=argparse.ArgumentParser();parser.add_argument('--workspace',required=True,type=Path)
    args=parser.parse_args();raise SystemExit(run(args.workspace.resolve()))
if __name__=='__main__':main()
