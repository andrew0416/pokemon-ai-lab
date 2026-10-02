"""Per-case fresh-process ABBA, then independent raw aggregate. No huge mutable ledger."""
from pathlib import Path,PurePosixPath
import argparse,os,time
import timing_contract as h
import timing_build as build
import timing_summary as summary
from timing_contract import c,base_ci
def result_name(index):return 'p7g-timing-'+str(index)+'-results'
def execute(workspace,evidence,arm,case,golden,stem,extra,deadline,cpu):
    row={'arm':arm,'case_id':case['id'],'source_sha':h.SOURCE,'plan_sha256':golden['sha256'],'status':'not_run_budget','lab_environment':{}}
    if deadline-time.monotonic()<h.CHILD_SECONDS:return row
    binary=h.binary(workspace,arm);digest=evidence['arms'][arm]['binary']['sha256']
    c.require(c.sha(binary)==digest,'Transferred binary changed')
    scenario=c.safe_file(workspace/'controller',case['scenario']);c.require(c.sha(scenario)==case['scenario_sha256'],'Frozen scenario changed')
    argv=[str(binary),str(scenario),'--joint-seed',str(case['joint_seed']),*extra]
    env=h.environment(workspace,arm);c.require(not any(k.startswith('LAB_') for k in env),'Observer environment forbidden')
    row.update(status='running',argv=argv,binary_sha256=digest)
    try:
        process=h.tail_process.run(argv,workspace/'source/engine',env,stem,cpu=cpu,timeout_seconds=h.CHILD_SECONDS,rss_limit_bytes=h.RSS)
        row.update(status=process['status'],process=process)
        for channel in ('stdout','stderr'):
            path=stem.with_suffix('.'+channel);row[channel+'_sha256']=c.sha(path);row[channel+'_bytes']=path.stat().st_size
    except Exception as error:row.update(status='execution_error',error=f'{type(error).__name__}: {error}')
    return row
def use_record(package,evidence,digest,cpu):
    return {'passed':True,'source_sha':h.SOURCE,'controller_sha':package['controller_sha'],'package_sha256':digest,'run_id':package['run_id'],'run_attempt':package['run_attempt'],'build_workspace':package['build_workspace'],'source_proof':evidence['source_proof'],'features':{a:h.features(a) for a in h.ARMS},'binary_sha256':{a:evidence['arms'][a]['binary']['sha256'] for a in h.ARMS},'fresh_named_tests':28,'cpu_affinity':[cpu]}
def verify_use(value,package,evidence,digest):
    cpu=value.get('cpu_affinity');c.require(isinstance(cpu,list) and len(cpu)==1 and type(cpu[0]) is int and cpu[0]>=0,'Invalid one-CPU affinity')
    c.require(value==use_record(package,evidence,digest,cpu[0]),'Mixed transferred build/run/source identity')
    return cpu[0]
def verify_child(row,folder,stem,arm,case,golden,extra,package,evidence,cpu):
    remote=PurePosixPath(package['build_workspace'])
    argv=[str(h.binary(remote,arm)),str(remote/'controller'/case['scenario']),'--joint-seed',str(case['joint_seed']),*extra]
    c.require(row['status']=='ok' and row['arm']==arm and row['case_id']==case['id'] and row['source_sha']==h.SOURCE and row['plan_sha256']==golden['sha256'] and row['binary_sha256']==evidence['arms'][arm]['binary']['sha256'],'Incomplete/mixed timing child')
    c.require(row['argv']==argv and row['lab_environment']=={},'Timing invocation/observer differs')
    p=row['process'];c.require(p['argv']==argv and p['status']=='ok' and p['returncode']==0 and 0<=p['wall_seconds']<h.CHILD_SECONDS and 0<=p['peak_rss_bytes']<=h.RSS and p['timeout_seconds']==h.CHILD_SECONDS and p['rss_limit_bytes']==h.RSS and p['cpu_affinity']==[cpu],'Invalid process completion/resource proof')
    for channel in ('stdout','stderr'):
        path=c.safe_file(folder,stem+'.'+channel)
        c.require(p[channel+'_file']==path.name and c.sha(path)==row[channel+'_sha256'] and path.stat().st_size==row[channel+'_bytes'],'Raw child stream changed')
def verify_case(folder,value,case,golden,package,evidence,cpu,index):
    c.require(value['status']=='passed' and value['case_id']==case['id'] and value['source_sha']==h.SOURCE and value['plan_sha256']==golden['sha256'] and set(value['descriptions'])==set(h.ARMS),'Incomplete/mixed case')
    plan=c.safe_file(folder,'plan.json')
    c.require(c.sha(plan)==golden['sha256'] and plan.stat().st_size==golden['bytes'],'Original B17 plan differs')
    parsed=c.line(plan);c.description(parsed,case)
    for arm,row in value['descriptions'].items():
        verify_child(row,folder,'describe-'+arm,arm,case,golden,['--describe'],package,evidence,cpu)
        c.require((folder/('describe-'+arm+'.stdout')).read_bytes()==plan.read_bytes(),'Description bytes differ')
    rows=value['rows'];schedule=summary.schedule();c.require(len(rows)==14,'Missing ABBA/warmup rows')
    remote_plan=PurePosixPath(package['build_workspace'])/result_name(index)/'cases'/case['id']/'plan.json'
    values=[];comparisons=[];first=None
    for number,(row,item) in enumerate(zip(rows,schedule)):
        c.require(all(row[k]==v for k,v in item.items()) and row['index']==number,'Changed warmup/ABBA order')
        stem=f'measurement-{number:02d}-{item["arm"]}'
        extra=['--plan',str(remote_plan),'--sample-seeds',','.join(map(str,case['sample_seeds']))]
        verify_child(row,folder,stem,item['arm'],case,golden,extra,package,evidence,cpu)
        actual=c.line(folder/(stem+'.stdout'));c.result(actual,parsed,case)
        if first is None:first=actual
        else:comparisons.append(h.s.p1.comparison(first,actual,parsed,case))
        values.append(actual['reference']['kernel_ns'])
    result=summary.case_summary(case['id'],values)
    c.require(result==value['summary'],'Stored timing ratio differs from raw kernel results')
    return result,comparisons
def timing(workspace,index,digest):
    root=workspace/result_name(index);root.mkdir(exist_ok=False);started=time.monotonic();deadline=started+h.SHARD_SECONDS
    selected=h.shard_cases(workspace,index);ledger={'status':'running','shard':index,'source_sha':h.SOURCE,'package_sha256':digest,'requested_cases':125,'completed_cases':0,'full500_complete':False,'cases':[{'case_id':case['id'],'status':'not_run'} for case,g in selected]}
    try:
        package,evidence=build.use_package(workspace,digest);bound=h.binding();h.source_proof(workspace,bound)
        cpu=min(os.sched_getaffinity(0));base_ci.write(root/'build-use.json',use_record(package,evidence,digest,cpu));base_ci.write(root/'machine.json',h.machine(cpu))
        for (case,golden),status in zip(selected,ledger['cases']):
            folder=root/'cases'/case['id'];folder.mkdir(parents=True,exist_ok=False)
            value={'case_id':case['id'],'source_sha':h.SOURCE,'plan_sha256':golden['sha256'],'status':'running','descriptions':{},'rows':[]}
            try:
                for arm in h.ARMS:
                    row=execute(workspace,evidence,arm,case,golden,folder/('describe-'+arm),['--describe'],deadline,cpu);value['descriptions'][arm]=row
                    if row['status']=='ok':
                        output=folder/('describe-'+arm+'.stdout');c.require(c.sha(output)==golden['sha256'] and output.stat().st_size==golden['bytes'],'Original B17 description differs');c.description(c.line(output),case)
                c.require(all(r['status']=='ok' for r in value['descriptions'].values()),'Description incomplete')
                plan=folder/'plan.json';plan.write_bytes((folder/'describe-off.stdout').read_bytes());values=[];parsed=c.line(plan);first=None
                for number,item in enumerate(summary.schedule()):
                    stem=folder/f'measurement-{number:02d}-{item["arm"]}'
                    row=execute(workspace,evidence,item['arm'],case,golden,stem,['--plan',str(plan),'--sample-seeds',','.join(map(str,case['sample_seeds']))],deadline,cpu)
                    row.update(item,index=number);value['rows'].append(row)
                    if row['status']=='ok':
                        try:
                            actual=c.line(stem.with_suffix('.stdout'));c.result(actual,parsed,case)
                            if first is None:first=actual
                            else:h.s.p1.comparison(first,actual,parsed,case)
                            values.append(actual['reference']['kernel_ns'])
                        except Exception as error:row.update(status='invalid_output',error=f'{type(error).__name__}: {error}')
                    base_ci.write(folder/'case.json',value)
                c.require(len(values)==14 and all(r['status']=='ok' for r in value['rows']),'ABBA/warmup incomplete')
                value.update(status='passed',summary=summary.case_summary(case['id'],values));ledger['completed_cases']+=1
            except Exception as error:value.update(status='failed_or_inconclusive',error=f'{type(error).__name__}: {error}')
            base_ci.write(folder/'case.json',value);status.update(status=value['status'],case_sha256=c.sha(folder/'case.json'));base_ci.write(root/'index.json',ledger)
        ledger['status']='passed' if ledger['completed_cases']==125 else 'failed_or_inconclusive'
        return 0 if ledger['status']=='passed' else 1
    except Exception as error:ledger.update(status='failed',error=f'{type(error).__name__}: {error}');return 1
    finally:
        ledger.update(wall_seconds=time.monotonic()-started,admission_budget_seconds=h.SHARD_SECONDS,child_timeout_seconds=h.CHILD_SECONDS,scope='Fresh-process first Full API call, not warmed in-process API. Earlier warmup processes warm OS/page caches only; each process also executes15 ancillary sampler calls.')
        base_ci.write(root/'index.json',ledger)
def aggregate(workspace,digest):
    root=workspace/'p7g-aggregate-results';root.mkdir(exist_ok=False)
    result={'status':'incomplete','full500_complete':False,'headline':None,'issues':[],'adoption_approved':False,'fresh_processes_expected':7000,'ancillary_sampler_calls_expected':105000,'describes_expected':1000}
    try:
        package,evidence=build.use_package(workspace,digest,False);h.binding();allcases=h.corpus(workspace);verified=[];machines=[]
        result.update(source_sha=h.SOURCE,controller_sha=package['controller_sha'],package_sha256=digest,reused_accuracy_audit_sha256=h.AUDIT_SHA)
        for index in range(4):
            folder=workspace/'downloaded'/f'engine-p7g-timing-{index}-{os.environ["GITHUB_RUN_ID"]}-1'
            if not (folder/'index.json').is_file():result['issues'].append({'shard':index,'reason':'missing artifact/index'});continue
            ledger=c.strict_json((folder/'index.json').read_bytes());expected=allcases[index::4]
            c.require(ledger['shard']==index and ledger['source_sha']==h.SOURCE and ledger['package_sha256']==digest and ledger['requested_cases']==125 and ledger['full500_complete'] is False and [v['case_id'] for v in ledger['cases']]==[case['id'] for case,g in expected],'Wrong shard identity/membership')
            use=c.strict_json((folder/'build-use.json').read_bytes());cpu=verify_use(use,package,evidence,digest)
            machine=c.strict_json((folder/'machine.json').read_bytes());c.require(machine['cpu_affinity']==[cpu] and machine.get('model_names') and machine['platform']=='linux','Missing CPU/platform identity');machines.append({'shard':index,**machine})
            count=0
            for status,(case,golden) in zip(ledger['cases'],expected):
                path=c.safe_file(folder,'cases/'+case['id']+'/case.json');c.require(c.sha(path)==status['case_sha256'],'Case record changed')
                value=c.strict_json(path.read_bytes());c.require(value['status']==status['status'] and value['case_id']==case['id'] and value['source_sha']==h.SOURCE and value['plan_sha256']==golden['sha256'],'Mixed case record')
                if value['status']!='passed':result['issues'].append({'case_id':case['id'],'status':value['status'],'error':value.get('error')});continue
                actual,_=verify_case(path.parent,value,case,golden,package,evidence,cpu,index);verified.append(actual);count+=1
            c.require(count==ledger['completed_cases'],'Shard completed count differs')
            if ledger['status']!='passed' or count!=125:result['issues'].append({'shard':index,'status':ledger['status'],'completed_cases':count})
        verified.sort(key=lambda v:v['case_id']);result.update(verified_cases=len(verified),completed_case_summaries=verified,machines=machines)
        if len(verified)==500 and not result['issues']:
            result.update(status='passed',full500_complete=True,headline=summary.whole_summary(verified,[case['id'] for case,g in allcases]))
        return 0 if result['full500_complete'] else 1
    except Exception as error:result.update(status='failed_validation',full500_complete=False,headline=None,error=f'{type(error).__name__}: {error}');return 1
    finally:
        result['scope']='Full500 fixed corpus paired Full API first-call timings. Equal-case ratio distribution and separate time-weighted workload totals; four VMs, no single-machine absolute total claim. Reuses exact same-source P7f full-joint correctness, not scalar metrics as a substitute.'
        base_ci.write(root/'summary.json',result)
        print('::notice::P7g timing '+result['status']+'; complete500='+str(result['full500_complete']))
def main():
    ap=argparse.ArgumentParser();ap.add_argument('stage',choices=('build','timing','aggregate'));ap.add_argument('--workspace',type=Path,required=True);ap.add_argument('--shard',type=int);ap.add_argument('--package-sha');a=ap.parse_args();workspace=a.workspace.resolve()
    if a.stage=='build':build.build(workspace)
    else:raise SystemExit(timing(workspace,a.shard,a.package_sha) if a.stage=='timing' else aggregate(workspace,a.package_sha))
if __name__=='__main__':main()
