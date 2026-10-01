"""Bounded accuracy first, then observer-free ABBA Full API timings."""
import argparse,math,os,statistics,time
from pathlib import Path
import pilot_contract as h
from pilot_contract import c,base_ci
import pilot_build as build
import tail_process

def inventory(root):return h.inherited_tail.inventory(root)
def has_budget(deadline):return deadline-time.monotonic()>=h.CHILD_SECONDS
def execute(workspace,built,arm,kind,case,plan,stem,deadline,extra):
    row={'arm':arm,'case_id':case['id'],'status':'not_run_phase_budget','source_sha':built['arms'][arm]['source_sha'],'plan_sha256':c.sha(plan),'lab_environment':{}}
    if not has_budget(deadline):return row
    binary=Path(built['arms'][arm]['binaries'][kind]['path'])
    c.require(c.sha(binary)==built['arms'][arm]['binaries'][kind]['sha256'],'Executable changed')
    scenario=c.safe_file(workspace/'controller',case['scenario'])
    c.require(c.sha(scenario)==case['scenario_sha256'],'Frozen scenario changed')
    argv=[str(binary),str(scenario),'--joint-seed',str(case['joint_seed']),*extra]
    row.update(status='running',argv=argv,binary_sha256=c.sha(binary))
    try:
        env=h.environment(workspace,arm);c.require(not any(k.startswith('LAB_') for k in env),'Observer environment forbidden')
        process=tail_process.run(argv,h.source_root(workspace,arm)/'engine',env,stem,cpu=min(os.sched_getaffinity(0)),timeout_seconds=h.CHILD_SECONDS,rss_limit_bytes=h.RSS)
        row.update(status=process['status'],process=process)
        for channel in ('stdout','stderr'):
            p=stem.with_suffix('.'+channel);row[channel+'_sha256']=c.sha(p);row[channel+'_bytes']=p.stat().st_size
    except Exception as error:row.update(status='execution_error',error=f'{type(error).__name__}: {error}')
    return row

def describe(workspace,built,arm,kind,case,plan,stem,deadline):
    row=execute(workspace,built,arm,kind,case,plan,stem,deadline,['--describe'])
    if row['status']=='ok':
        try:
            c.require(stem.with_suffix('.stdout').read_bytes()==plan.read_bytes(),'Description bytes changed')
            c.description(c.line(stem.with_suffix('.stdout')),case)
        except Exception as error:row.update(status='invalid_output',error=f'{type(error).__name__}: {error}')
    return row

def correctness(workspace):
    folder=workspace/h.RESULTS;path=folder/'correctness.json';c.require(not path.exists(),'Correctness already attempted')
    value={'status':'running','cases':[],'full_state_agreement':False,'full500_complete':False,'adoption_approved':False}
    started=time.monotonic();deadline=started+h.PHASE_SECONDS
    try:
        b=h.binding();h.verify_sources(workspace,b);built=build.verify_build(workspace,b);_,cases=h.pilot(workspace)
        value.update(source_sha=b['source_sha'],reference_sha=h.REFERENCE,source_binding_sha256=c.sha(h.HERE/'source-binding.json'))
        for index,(case,plan) in enumerate(cases):
            row={'case_id':case['id'],'plan_sha256':c.sha(plan),'arms':{},'comparisons':{},'status':'running'}
            value['cases'].append(row);base_ci.write(path,value)
            order=h.ARMS[index%3:]+h.ARMS[:index%3]
            row['arm_order']=list(order)
            for arm in order:
                directory=folder/'accuracy'/case['id']/arm;directory.mkdir(parents=True,exist_ok=False)
                desc=describe(workspace,built,arm,'joint',case,plan,directory/'describe',deadline)
                entry={'description':desc,'status':'description_failed'}
                row['arms'][arm]=entry;base_ci.write(path,value)
                if desc['status']!='ok':continue
                output=directory/'export';stem=directory/'execution'
                measured=execute(workspace,built,arm,'joint',case,plan,stem,deadline,['--plan',str(plan),'--output-dir',str(output),'--max-rows',str(h.compare_joint.MAX_ROWS),'--max-bytes',str(h.compare_joint.MAX_BYTES)])
                entry.update(status=measured['status'],execution=measured,export_files=inventory(output))
                if measured['status']=='ok':
                    try:
                        actual=h.compare_joint.read_export(output,plan)
                        c.require(c.strict_json(stem.with_suffix('.stdout').read_bytes())==actual['manifest'],'Export stdout differs from manifest')
                        entry.update(status='complete',manifest=actual['manifest'],file_sha256=actual['file_sha256'],validated_rows=actual['rows'],raw_mass=actual['mass'])
                    except Exception as error:entry.update(status='invalid_output',error=f'{type(error).__name__}: {error}')
                base_ci.write(path,value)
            row['status']='incomplete'
            if all(v['status']=='complete' for v in row['arms'].values()) and set(row['arms'])==set(h.ARMS):
                try:
                    base=folder/'accuracy'/case['id']
                    for arm in ('off','on'):
                        proof=h.compare_joint.compare_exports(base/'reference/export',base/arm/'export',plan)
                        if arm=='off':
                            payloads=('dictionary.json','joint.bin','selection.json')
                            c.require(all(proof['baseline_file_sha256'][name]==proof['candidate_file_sha256'][name] for name in payloads),'Feature-OFF changed reference payload bytes')
                            proof['reference_off_payload_bytes_exact']=True
                            proof['byte_exact_payloads']=list(payloads)
                        row['comparisons']['reference_vs_'+arm]=proof
                    row['status']='passed'
                except Exception as error:row.update(status='mismatch',error=f'{type(error).__name__}: {error}')
            base_ci.write(path,value)
        value['full_state_agreement']=all(row['status']=='passed' for row in value['cases']) and [r['case_id'] for r in value['cases']]==h.CASE_IDS
        value['status']='passed' if value['full_state_agreement'] else 'failed_or_inconclusive'
        return 0 if value['full_state_agreement'] else 1
    except Exception as error:value.update(status='failed',full_state_agreement=False,error=f'{type(error).__name__}: {error}');return 1
    finally:
        value.update(phase_wall_seconds=time.monotonic()-started,phase_budget_seconds=h.PHASE_SECONDS,budget_admission='Start a child only with at least its full 300s cap remaining. Validation/bookkeeping may extend beyond deadline.',scope='Full State + full Suspension joint support for eight fixed decisions; per-key 1e-12 and normalized TV/raw mass 1e-9. Reference563 is P1eON, not original a4 oracle.')
        base_ci.write(path,value)

def timing_schedule():
    result=[]
    for case in h.TIMED_IDS:
        for arm in ('off','on'):result.append({'case_id':case,'phase':'warmup','block':None,'slot':None,'arm':arm})
    for block in range(3):
        for case in h.TIMED_IDS:
            for slot,arm in enumerate(('off','on','on','off')):
                result.append({'case_id':case,'phase':'timed','block':block,'slot':slot,'arm':arm})
    return result

def timing_summary(rows,b,plans):
    expected=timing_schedule()
    c.require(len(rows)==len(expected),'Incomplete timing schedule')
    for row,want in zip(rows,expected):
        c.require(all(row.get(k)==v for k,v in want.items()),'Changed ABBA order/identity')
        c.require(row.get('source_sha')==b['source_sha'],'Mixed source timing row')
    complete=all(row.get('status')=='ok' for row in rows)
    result={'complete':complete,'by_case':{},'ratio_metric':'Full factored API reference.kernel_ns, ON/OFF on identical new source','full500_complete':False,'adoption_approved':False,'timeout_ratio':None}
    if not complete:return result
    for case_id in h.TIMED_IDS:
        case,plan=plans[case_id];blocks=[]
        for block in range(3):
            group=[r for r in rows if r['phase']=='timed' and r['case_id']==case_id and r['block']==block]
            c.require([r['arm'] for r in group]==['off','on','on','off'],'ABBA block incomplete')
            comparisons=[h.s.p1.comparison(group[0]['value'],r['value'],c.line(plan),case) for r in group[1:]]
            off=[r['value']['reference']['kernel_ns'] for r in group if r['arm']=='off']
            on=[r['value']['reference']['kernel_ns'] for r in group if r['arm']=='on']
            blocks.append({'block':block,'off_kernel_ns':off,'on_kernel_ns':on,'on_over_off':sum(on)/sum(off),'derived_metric_comparisons':comparisons})
        ratios=[x['on_over_off'] for x in blocks];ratio=statistics.median(ratios)
        result['by_case'][case_id]={'blocks':blocks,'median_on_over_off':ratio,'median_reduction_percent':100*(1-ratio),'statistical_significance_claim':False}
    return result

def timings(workspace):
    folder=workspace/h.RESULTS;path=folder/'timings.json';c.require(not path.exists(),'Timing already attempted')
    value={'status':'running','rows':[],'descriptions':[],'adoption_approved':False}
    started=time.monotonic();deadline=started+h.PHASE_SECONDS
    try:
        b=h.binding();h.verify_sources(workspace,b);built=build.verify_build(workspace,b);_,cases=h.pilot(workspace);plans={case['id']:(case,plan) for case,plan in cases}
        gate=c.strict_json((folder/'correctness.json').read_bytes())
        c.require(gate['status']=='passed' and gate['full_state_agreement'] is True and gate['source_sha']==b['source_sha'] and gate['reference_sha']==h.REFERENCE and gate['source_binding_sha256']==c.sha(h.HERE/'source-binding.json'),'Exact pilot correctness gate missing/mixed')
        c.require([r['case_id'] for r in gate['cases']]==h.CASE_IDS and all(r['status']=='passed' for r in gate['cases']),'Incomplete correctness case gate')
        value.update(source_sha=b['source_sha'],reference_sha=h.REFERENCE,correctness_sha256=c.sha(folder/'correctness.json'),source_binding_sha256=c.sha(h.HERE/'source-binding.json'),observer=False)
        for case_id in h.TIMED_IDS:
            case,plan=plans[case_id]
            for arm in ('off','on'):
                directory=folder/'timing-describes'/case_id/arm;directory.mkdir(parents=True,exist_ok=False)
                row=describe(workspace,built,arm,'benchmark',case,plan,directory/'description',deadline)
                value['descriptions'].append(row);base_ci.write(path,value)
                c.require(row['status']=='ok','Benchmark selection failed before timing')
        for index,item in enumerate(timing_schedule()):
            case,plan=plans[item['case_id']];directory=folder/'timing-runs'/f'{index:03d}-{item["case_id"]}-{item["arm"]}';directory.mkdir(parents=True,exist_ok=False)
            stem=directory/'measurement'
            row=execute(workspace,built,item['arm'],'benchmark',case,plan,stem,deadline,['--plan',str(plan),'--sample-seeds',','.join(map(str,case['sample_seeds']))])
            row.update(item);value['rows'].append(row)
            if row['status']=='ok':
                try:
                    result=c.line(stem.with_suffix('.stdout'));c.result(result,c.line(plan),case);row['value']=result
                except Exception as error:row.update(status='invalid_output',error=f'{type(error).__name__}: {error}')
            base_ci.write(path,value)
        value['summary']=timing_summary(value['rows'],b,plans)
        value['status']='passed' if value['summary']['complete'] else 'failed_or_inconclusive'
        return 0 if value['summary']['complete'] else 1
    except Exception as error:value.update(status='failed',error=f'{type(error).__name__}: {error}');return 1
    finally:
        value.update(phase_wall_seconds=time.monotonic()-started,phase_budget_seconds=h.PHASE_SECONDS,
          scope='Five preselected cases, one OFF/ON warmup discarded, three ABBA blocks. API kernel only; process CPU/RSS and sampler metrics are retained as ancillary data. No general speedup/global correctness/adoption claim.')
        base_ci.write(path,value)
        print('::notice::P7d pilot timing '+value['status']+'; see raw per-case Full API measurements. No full500/adoption claim.')
        summary=os.environ.get('GITHUB_STEP_SUMMARY')
        if summary:
            with Path(summary).open('a',encoding='utf8') as stream:stream.write('## P7d pilot\n\nStatus: '+value['status']+'. Five-case Full API ABBA timing after eight-case joint correctness. No full500 or adoption claim.\n')

def main():
    parser=argparse.ArgumentParser();parser.add_argument('stage',choices=('prepare','build','correctness','timings'));parser.add_argument('--workspace',required=True,type=Path);args=parser.parse_args()
    function=getattr(build,args.stage) if args.stage in ('prepare','build') else globals()[args.stage]
    result=function(args.workspace.resolve())
    if args.stage in ('correctness','timings'):raise SystemExit(result)
if __name__=='__main__':main()
