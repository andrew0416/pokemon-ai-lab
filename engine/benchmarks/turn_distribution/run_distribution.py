"""Freeze all opening descriptions, then measure the preselected prefix without replacements."""
import argparse
from collections import Counter
import json
import math
import os
from pathlib import Path
import statistics
import time
import ci
import contract as c
import process_run

def summary(records,requested=c.COUNT):
    c.require(len(records)==500 and [r['id'] for r in records]==[f'opening-{i:04d}' for i in range(500)],'Incomplete or reordered ledger')
    desc=Counter(r['description']['status'] for r in records)
    selected=records[:requested];status=Counter(r['measurement']['status'] for r in selected)
    complete=desc=={'ok':500} and status=={'ok':requested}
    result={'schema':1,'complete':complete,'scope':'all 500 frozen openings measured on one runner',
        'endpoint_scope':'next decision boundary; intermediate switch suspensions retained (not every case is end-of-turn)',
        'requested_cases':requested,'corpus_cases':500,'requested_descriptions':500,
        'description_status_counts':dict(desc),'measurement_status_counts':dict(status),
        'case_ids':[r['id'] for r in selected],
        'failed_descriptions':[r['id'] for r in records if r['description']['status']!='ok'],
        'failed_measurements':[r['id'] for r in selected if r['measurement']['status']!='ok'],
        'official_metrics':None,'partial_success_metrics_reported_as_complete':False}
    if not complete:return result
    values=[r['value'] for r in selected]
    result['suspended_reference_case_count']=sum(v['reference']['suspended_components']>0 for v in values)
    def describe(numbers):
        numbers=sorted(numbers);return {'n':len(numbers),'mean':statistics.mean(numbers),'median':statistics.median(numbers),
            'p95_nearest_rank':numbers[math.ceil(.95*len(numbers))-1],'min':numbers[0],'max':numbers[-1]}
    metrics={'time_unit':'nanoseconds','reference_kernel_ns':describe([v['reference']['kernel_ns'] for v in values]),
        'reference_components':describe([v['reference']['components'] for v in values]),
        'reference_metric_prepare_ns':describe([v['reference']['metric_prepare_ns'] for v in values]),
        'top32_retained_mass':describe([v['non_hp_state_posthoc_top32']['retained_mass'] for v in values]),'samples':{}}
    for count in c.SAMPLE_COUNTS:
        # Opening-level means avoid counting five dependent budgets as five openings.
        sets=[[s for s in value['samples'] if s['count']==count] for value in values]
        metrics['samples'][str(count)]={'openings':requested,'seeds_per_opening':5,
            'kernel_ns_opening_mean':describe([statistics.mean(s['kernel_ns'] for s in rows) for rows in sets]),
            'metric_ns_opening_mean':describe([statistics.mean(s['metric_ns'] for s in rows) for rows in sets])}
        for kind in ('full_state','non_hp_state'):
            metrics['samples'][str(count)][kind]={key:describe([statistics.mean(s[kind][key] for s in rows) for rows in sets])
                for key in ('coverage','tv','outside_reference_mass','unique_states')}
    result['official_metrics']=metrics
    seconds=sum(r['measurement']['process']['wall_seconds'] for r in selected)
    result['process_measurement_wall_seconds_sum']=seconds
    return result

def run(workspace):
    result=workspace/'turn-distribution-results';receipt=c.strict_json((result/'build-receipt.json').read_bytes())
    c.require(receipt['status']=='success' and receipt['cached_regression_reused'] is False
        and receipt['source_sha']==c.SOURCE_SHA and receipt['corpus_sha256']==c.CORPUS_SHA
        and receipt['features']==list(c.CORE_FEATURES),'Fresh pinned build required')
    ci.verify_source(workspace/'source');manifest=c.corpus(workspace/'controller');env=ci.environment(workspace)
    binary=workspace/'target-distribution/release/lab-distribution-bench'
    c.require(c.sha(binary)==receipt['binary_sha256'] and Path(receipt['binary'])==binary,'Measured executable differs')
    out=result/'cases';out.mkdir(exist_ok=False);cpu=min(os.sched_getaffinity(0))
    records=[{'id':case['id'],'description':{'status':'pending'},
        'measurement':{'status':'pending' if i<c.COUNT else 'not_selected'}} for i,case in enumerate(manifest['cases'])]
    def save():
        ci.write(result/'records.json',{'schema':1,'source_sha':c.SOURCE_SHA,'corpus_sha256':c.CORPUS_SHA,
            'binary_sha256':receipt['binary_sha256'],'prefix_count':c.COUNT,'cases':records})
        ci.write(result/'summary.json',summary(records,c.COUNT))
    save()
    for phase in ('description','measurement'):
        if phase=='measurement' and any(r['description']['status']!='ok' for r in records):
            for r in records[:c.COUNT]:r['measurement']={'status':'not_run_description_failure'}
            save();return 1
        cases=manifest['cases'] if phase=='description' else manifest['cases'][:c.COUNT]
        for case,row in zip(cases,records):
            directory=out/case['id'];directory.mkdir(exist_ok=True);stem=directory/phase
            scenario=c.safe_file(workspace/'controller',case['scenario'])
            c.require(c.sha(scenario)==case['scenario_sha256'] and c.sha(binary)==receipt['binary_sha256'],'Input changed before child execution')
            argv=[str(binary),str(scenario),'--joint-seed',str(case['joint_seed'])]
            if phase=='description':argv+=['--describe']
            else:
                plan=directory/'description.stdout';c.require(c.sha(plan)==row['description']['stdout_sha256'],'Frozen opening plan changed')
                argv+=['--plan',str(plan),'--sample-seeds',','.join(map(str,case['sample_seeds']))]
            entry={'status':'running'};row[phase]=entry;save()
            try:
                process=process_run.run(argv,workspace/'source/engine',env,stem,cpu=cpu)
                entry.update(process=process,status=process['status'])
                for stream in ('stdout','stderr'):
                    path=stem.with_suffix('.'+stream);entry[stream+'_sha256']=c.sha(path);entry[stream+'_bytes']=path.stat().st_size
                if process['status']=='ok':
                    value=c.line(stem.with_suffix('.stdout'))
                    if phase=='description':c.description(value,case)
                    else:
                        c.result(value,c.line(directory/'description.stdout'),case);row['value']=value
            except Exception as error:
                entry.update(status='validation_or_execution_error',error=f'{type(error).__name__}: {error}')
            save()
    return 0 if summary(records,c.COUNT)['complete'] else 1

def main():
    parser=argparse.ArgumentParser();parser.add_argument('--workspace',type=Path,required=True)
    args=parser.parse_args();raise SystemExit(run(args.workspace.resolve()))
if __name__=='__main__':main()
