"""Full500 and broad104 accuracy only; every requested case gets an explicit terminal record."""
from pathlib import Path, PurePosixPath
import argparse,os,time
import suffix_contract as h
import suffix_build as build
from suffix_contract import c,base_ci

def run_name(kind,index=None):return 'p7h-'+('full500-'+str(index) if kind=='full500' else 'broad')+'-results'
def process(workspace,evidence,arm,kind,argv_tail,stem,deadline,overrides=None):
    row={'arm':arm,'source_sha':evidence['arms'][arm]['source_sha'],'status':'not_run_budget','lab_environment':dict(overrides or {})}
    if deadline-time.monotonic()<h.CHILD_SECONDS:return row
    binary=h.package_bin(workspace,arm,kind);digest=evidence['arms'][arm]['binaries'][kind]['sha256']
    c.require(c.sha(binary)==digest,'Transferred executable changed')
    env=h.environment(workspace,arm);env.update(overrides or {});argv=[str(binary),*argv_tail]
    row.update(status='running',argv=argv,binary_sha256=digest)
    try:
        value=h.tail_process.run(argv,h.source_root(workspace,arm)/'engine',env,stem,cpu=min(os.sched_getaffinity(0)),timeout_seconds=h.CHILD_SECONDS,rss_limit_bytes=h.RSS)
        row.update(status=value['status'],process=value)
        for channel in ('stdout','stderr'):
            path=stem.with_suffix('.'+channel);row[channel+'_sha256']=c.sha(path);row[channel+'_bytes']=path.stat().st_size
    except Exception as error:row.update(status='execution_error',error=f'{type(error).__name__}: {error}')
    return row

def preflight(workspace,package_sha,root):
    package,evidence=build.use_package(workspace,package_sha);bound=h.binding();proof=h.source_fingerprint(workspace,bound)
    base_ci.write(root/'build-use.json',{'passed':True,'package_sha256':package_sha,'source_proof':proof,'controller_sha':package['controller_sha'],'source_sha':bound['source_sha'],'reference_sha':h.REFERENCE,'features':{a:h.features(a) for a in h.ARMS},'run_id':package['run_id'],'run_attempt':package['run_attempt'],'fresh_build_named_tests':h.named_count(bound),'binaries':{a:evidence['arms'][a]['binaries'] for a in h.ARMS},'actual_cpu_affinity':[min(os.sched_getaffinity(0))]})
    return package,evidence,bound

def finish(root,value,started):
    value.update(wall_seconds=time.monotonic()-started,accuracy_only=True,adoption_approved=False,performance_claim=False)
    base_ci.write(root/'ledger.json',value)
    base_ci.write(root/'summary.json',{key:value.get(key) for key in ('kind','shard','status','source_sha','reference_sha','package_sha256','requested_cases','completed_cases','full500_complete','broad104_complete','wall_seconds','accuracy_only','adoption_approved','performance_claim','error')})

def full500(workspace,index,package_sha):
    root=workspace/run_name('full500',index);root.mkdir(exist_ok=False);started=time.monotonic();deadline=started+h.SHARD_SECONDS
    cases=h.shard_cases(workspace,index)
    value={'kind':'full500','shard':index,'status':'running','source_sha':h.binding()['source_sha'],'reference_sha':h.REFERENCE,'package_sha256':package_sha,'requested_cases':len(cases),'completed_cases':0,'full500_complete':False,
      'cases':[{'case_id':case['id'],'source_sha':h.binding()['source_sha'],'plan_sha256':golden['sha256'],'status':'not_run','arms':{},'comparisons':{}} for case,golden in cases]}
    try:
        _,evidence,bound=preflight(workspace,package_sha,root)
        base_ci.write(root/'ledger.json',value)
        for (case,golden),row in zip(cases,value['cases']):
            if deadline-time.monotonic()<h.CHILD_SECONDS:row['status']='not_run_budget';continue
            directory=root/'cases'/case['id'];directory.mkdir(parents=True,exist_ok=False)
            scenario=c.safe_file(workspace/'controller',case['scenario']);args=[str(scenario),'--joint-seed',str(case['joint_seed'])]
            for arm in h.ARMS:
                folder=directory/arm;folder.mkdir()
                description=process(workspace,evidence,arm,'joint',args+['--describe'],folder/'describe',deadline)
                row['arms'][arm]={'status':'description_failed','description':description}
                if description['status']=='ok':
                    try:
                        raw=folder/'describe.stdout';c.require(c.sha(raw)==golden['sha256'] and raw.stat().st_size==golden['bytes'],'Original frozen B17 description differs')
                        c.description(c.line(raw),case);row['arms'][arm]['status']='described'
                    except Exception as error:description.update(status='invalid_output',error=f'{type(error).__name__}: {error}')
                base_ci.write(root/'ledger.json',value)
            if row['arms']['reference']['status']!='described':
                row['status']='reference_description_failed';continue
            plan=directory/'reference/describe.stdout'
            for arm in h.ARMS:
                entry=row['arms'][arm];folder=directory/arm;output=folder/'export'
                if entry['status']!='described':continue
                c.require((folder/'describe.stdout').read_bytes()==plan.read_bytes(),'Hash-approved descriptions differ')
                argv=args+['--plan',str(plan),'--output-dir',str(output),'--max-rows',str(h.compare_joint.MAX_ROWS),'--max-bytes',str(h.compare_joint.MAX_BYTES)]
                execution=process(workspace,evidence,arm,'joint',argv,folder/'execution',deadline)
                entry.update(status=execution['status'],execution=execution,export_files=h.prior.inherited_tail.inventory(output))
                if execution['status']=='ok':
                    try:
                        actual=h.compare_joint.read_export(output,plan)
                        c.require(c.strict_json((folder/'execution.stdout').read_bytes())==actual['manifest'],'Exporter stdout/manifest mismatch')
                        entry.update(status='complete',manifest=actual['manifest'],file_sha256=actual['file_sha256'],joint_rows=actual['rows'],raw_mass=actual['mass'])
                    except Exception as error:entry.update(status='invalid_output',error=f'{type(error).__name__}: {error}')
                base_ci.write(root/'ledger.json',value)
            row['status']='incomplete'
            if all(entry['status']=='complete' for entry in row['arms'].values()):
                try:
                    for arm in ('off','on'):
                        proof=h.compare_joint.compare_exports(directory/'reference/export',directory/arm/'export',plan)
                        if arm=='off':
                            names=('dictionary.json','joint.bin','selection.json')
                            c.require(all(proof['baseline_file_sha256'][n]==proof['candidate_file_sha256'][n] for n in names),'Feature-OFF reference payload bytes differ')
                            proof['reference_off_payload_bytes_exact']=True
                        row['comparisons']['reference_vs_'+arm]=proof
                    row['status']='passed';value['completed_cases']+=1
                except Exception as error:row.update(status='mismatch',error=f'{type(error).__name__}: {error}')
            base_ci.write(root/'ledger.json',value)
        value['status']='passed' if value['completed_cases']==value['requested_cases']==125 else 'failed_or_inconclusive'
        return 0 if value['status']=='passed' else 1
    except Exception as error:value.update(status='failed',error=f'{type(error).__name__}: {error}');return 1
    finally:
        value['scope']='One disjoint 125-case subset of all500 frozen B17 Full decisions. Full State/Suspension plus all12 joint HP; reference87e7 with P1e/P7d ON versus new source suffix OFF/ON. This shard alone never claims full500 completion.'
        value['budget_policy']='1500s admission budget, 300s complete child allowance required before start; validation/bookkeeping may continue after deadline.'
        finish(root,value,started)

def broad(workspace,package_sha):
    root=workspace/run_name('broad');root.mkdir(exist_ok=False);started=time.monotonic();deadline=started+h.SHARD_SECONDS
    value={'kind':'broad','status':'running','source_sha':h.binding()['source_sha'],'reference_sha':h.REFERENCE,'package_sha256':package_sha,'requested_cases':104,'completed_cases':0,'broad104_complete':False,'arms':{},'comparisons':{}}
    try:
        _,evidence,bound=preflight(workspace,package_sha,root);contract=h.broad_contract()
        base_ci.write(root/'corpus-manifest.json',contract)
        for arm in h.ARMS:
            directory=root/arm;directory.mkdir();records=directory/'records.jsonl';stem=directory/'execution'
            overrides={**h.s.SCOPE_ENV,h.s.BROAD_ENV:str(records)}
            argv=['p1e2_full_state_corpus','--exact','--test-threads=1','--show-output']
            row=process(workspace,evidence,arm,'broad',argv,stem,deadline,overrides);value['arms'][arm]=row
            if records.is_file():row.update(records_sha256=c.sha(records),records_bytes=records.stat().st_size)
            if row['status']=='ok':
                try:
                    row['test_proof']=h.s.old_ci.named_test_proof(stem.with_suffix('.stdout').read_text(encoding='utf8'),['p1e2_full_state_corpus'],1)
                    c.require(records.is_file() and not records.is_symlink(),'Fresh broad records missing')
                    row['record_validation']=h.compare_broad.compare_records(records,records,contract);row['status']='complete'
                except Exception as error:row.update(status='invalid_output',error=f'{type(error).__name__}: {error}')
            base_ci.write(root/'ledger.json',value)
        if all(row['status']=='complete' for row in value['arms'].values()):
            for arm in ('off','on'):value['comparisons']['reference_vs_'+arm]=h.compare_broad.compare_records(root/'reference/records.jsonl',root/arm/'records.jsonl',contract)
            value.update(status='passed',completed_cases=104,broad104_complete=True,fresh_named_test_executions=3)
        else:value['status']='failed_or_inconclusive'
        return 0 if value['broad104_complete'] else 1
    except Exception as error:value.update(status='failed',error=f'{type(error).__name__}: {error}');return 1
    finally:
        value['scope']='104 unchanged mechanics cases, mostly Extremes and one single-target Full case. This corpus does not independently prove new suffix checkpoint activation; fresh named Full core tests provide that evidence.'
        finish(root,value,started)

def verify_build_use(folder,package,evidence,bound):
    use=c.strict_json((folder/'build-use.json').read_bytes())
    expected={'passed':True,'package_sha256':c.sha(folder.parents[1]/h.PACKAGE/'package.json'),
      'source_proof':evidence['source_proof'],'controller_sha':package['controller_sha'],'source_sha':bound['source_sha'],
      'reference_sha':h.REFERENCE,'features':{a:h.features(a) for a in h.ARMS},'run_id':package['run_id'],
      'run_attempt':package['run_attempt'],'fresh_build_named_tests':h.named_count(bound),
      'binaries':{a:evidence['arms'][a]['binaries'] for a in h.ARMS},'actual_cpu_affinity':use.get('actual_cpu_affinity')}
    c.require(use==expected and isinstance(use['actual_cpu_affinity'],list) and len(use['actual_cpu_affinity'])==1 and type(use['actual_cpu_affinity'][0]) is int,'Mixed build-use identity')
    return use

def verify_process(record,folder,label,arm,kind,tail,lab,package,evidence,use,complete_status='ok'):
    remote=PurePosixPath(package['build_workspace'])
    argv=[str(h.package_bin(remote,arm,kind)),*tail]
    c.require(record['arm']==arm and record['source_sha']==evidence['arms'][arm]['source_sha'] and record['binary_sha256']==evidence['arms'][arm]['binaries'][kind]['sha256'],'Mixed executable/source identity')
    c.require(record['status']==complete_status and record['argv']==argv and record['lab_environment']==lab,'Child invocation differs')
    process_=record['process']
    c.require(process_['argv']==argv and process_['status']=='ok' and process_['returncode']==0 and 0<=process_['wall_seconds']<h.CHILD_SECONDS and 0<=process_['peak_rss_bytes']<=h.RSS and process_['rss_limit_bytes']==h.RSS and process_['timeout_seconds']==h.CHILD_SECONDS and process_['cpu_affinity']==use['actual_cpu_affinity'],'Invalid completed process/bound/CPU')
    for channel in ('stdout','stderr'):
        file=c.safe_file(folder,label+'.'+channel)
        c.require(c.sha(file)==record[channel+'_sha256'] and file.stat().st_size==record[channel+'_bytes'],'Raw child stream changed')
        c.require(Path(process_[channel+'_file']).name==label+'.'+channel,'Raw child stream identity differs')

def verify_case_files(directory,row,case,golden,package,evidence,use,index):
    c.require(row['status']=='passed' and set(row['arms'])==set(h.ARMS) and set(row['comparisons'])=={'reference_vs_off','reference_vs_on'},'Incomplete full500 case')
    c.require(row['case_id']==case['id'] and row['source_sha']==package['source_sha'] and row['plan_sha256']==golden['sha256'],'Mixed case/source/plan')
    remote=PurePosixPath(package['build_workspace']);remote_case=remote/run_name('full500',index)/'cases'/case['id']
    args=[str(remote/'controller'/case['scenario']),'--joint-seed',str(case['joint_seed'])]
    plan=directory/'reference/describe.stdout';remote_plan=remote_case/'reference/describe.stdout'
    c.require(c.sha(plan)==golden['sha256'] and plan.stat().st_size==golden['bytes'],'Original B17 frozen plan differs')
    actual={}
    for arm,entry in row['arms'].items():
        c.require(entry['status']=='complete','Incomplete arm claimed pass')
        folder=directory/arm
        verify_process(entry['description'],folder,'describe',arm,'joint',args+['--describe'],{},package,evidence,use)
        c.require((folder/'describe.stdout').read_bytes()==plan.read_bytes(),'Cross-arm descriptions differ')
        c.description(c.line(folder/'describe.stdout'),case)
        tail=args+['--plan',str(remote_plan),'--output-dir',str(remote_case/arm/'export'),'--max-rows',str(h.compare_joint.MAX_ROWS),'--max-bytes',str(h.compare_joint.MAX_BYTES)]
        verify_process(entry['execution'],folder,'execution',arm,'joint',tail,{},package,evidence,use)
        export=folder/'export';actual[arm]=h.compare_joint.read_export(export,plan)
        c.require(c.strict_json((folder/'execution.stdout').read_bytes())==actual[arm]['manifest']==entry['manifest'],'Exporter manifest/stdout differs')
        c.require(actual[arm]['file_sha256']==entry['file_sha256'] and actual[arm]['rows']==entry['joint_rows'] and actual[arm]['mass']==entry['raw_mass'],'Export ledger differs')
        c.require(h.prior.inherited_tail.inventory(export)==entry['export_files'],'Export inventory differs')
    for arm in ('off','on'):
        saved=row['comparisons']['reference_vs_'+arm]
        c.require(saved['baseline_file_sha256']==actual['reference']['file_sha256'] and saved['candidate_file_sha256']==actual[arm]['file_sha256'],'Comparison references other payloads')
    names=('dictionary.json','joint.bin','selection.json')
    c.require(all(actual['reference']['file_sha256'][name]==actual['off']['file_sha256'][name] for name in names),'Feature-OFF reference payload bytes differ')
    saved_off=row['comparisons']['reference_vs_off']
    c.require(saved_off['passed'] is True and saved_off['reference_off_payload_bytes_exact'] is True and saved_off['full_non_hp_state_suspension_dictionary_exact'] is True and saved_off['full_joint_party_hp_support_exact'] is True and saved_off['max_abs_probability_error']==0 and saved_off['normalized_tv']==0 and saved_off['unique_joint_rows']==actual['reference']['rows'],'OFF comparison differs from identical raw payloads')
    recomputed=h.compare_joint.compare_exports(directory/'reference/export',directory/'on/export',plan)
    c.require(recomputed==row['comparisons']['reference_vs_on'],'Raw full joint comparison differs')
    return recomputed

def aggregate(workspace,package_sha):
    root=workspace/'p7h-aggregate-results';root.mkdir(exist_ok=False);path=root/'summary.json'
    value={'status':'incomplete','full500_complete':False,'broad104_complete':False,'adoption_approved':False,'accuracy_only':True,'performance_claim':False,'issues':[]}
    try:
        package,evidence=build.use_package(workspace,package_sha,False);bound=h.binding();cases=h.corpus(workspace);seen=[];support=0;maxerror=0.;shards=[]
        value.update(source_sha=bound['source_sha'],reference_sha=h.REFERENCE,package_sha256=package_sha,controller_sha=package['controller_sha'],fresh_build_named_tests=h.named_count(bound))
        for index in range(h.PARTITIONS):
            folder=workspace/'downloaded'/('engine-p7h-accuracy-full500-'+str(index)+'-'+os.environ['GITHUB_RUN_ID']+'-1')
            ledger=folder/'ledger.json'
            if not ledger.is_file():value['issues'].append({'shard':index,'reason':'missing artifact/ledger'});continue
            v=c.strict_json(ledger.read_bytes());expected=[case['id'] for case,golden in h.shard_cases(workspace,index)]
            c.require(v['kind']=='full500' and v['shard']==index and v['source_sha']==bound['source_sha'] and v['reference_sha']==h.REFERENCE and v['package_sha256']==package_sha,'Wrong shard identity')
            c.require([r['case_id'] for r in v['cases']]==expected and v['requested_cases']==125,'Missing/reordered shard members')
            use=verify_build_use(folder,package,evidence,bound)
            shards.append({'index':index,'status':v['status'],'ledger_sha256':c.sha(ledger),'completed_cases':v['completed_cases']})
            for row in v['cases']:
                if row['status']!='passed':value['issues'].append({'case_id':row['case_id'],'reason':row['status']});continue
                case,golden=cases[int(row['case_id'].split('-')[1])]
                c.require(row['source_sha']==bound['source_sha'] and row['plan_sha256']==golden['sha256'],'Mixed source/plan case')
                directory=folder/'cases'/case['id'];proof=verify_case_files(directory,row,case,golden,package,evidence,use,index)
                seen.append(case['id']);support+=proof['unique_joint_rows'];maxerror=max(maxerror,proof['max_abs_probability_error'])
            if v['status']!='passed' or v['completed_cases']!=125:value['issues'].append({'shard':index,'reason':'incomplete shard'})
        c.require(len(seen)==len(set(seen)),'Duplicate case across shards')
        value.update(shards=shards,verified_full500_cases=len(seen),sum_per_case_joint_support=support,max_per_key_abs_probability_error=maxerror)
        value['full500_complete']=sorted(seen)==[case['id'] for case,golden in cases] and len(shards)==4 and all(x['status']=='passed' and x['completed_cases']==125 for x in shards)
        folder=workspace/'downloaded'/('engine-p7h-accuracy-broad-'+os.environ['GITHUB_RUN_ID']+'-1')
        if (folder/'ledger.json').is_file():
            v=c.strict_json((folder/'ledger.json').read_bytes())
            c.require(v['kind']=='broad' and v['source_sha']==bound['source_sha'] and v['reference_sha']==h.REFERENCE and v['package_sha256']==package_sha,'Wrong broad identity')
            if v['status']=='passed' and v['broad104_complete'] is True:
                c.require(set(v['arms'])==set(h.ARMS) and v['requested_cases']==v['completed_cases']==104 and v['fresh_named_test_executions']==3,'Missing broad arm/cases')
                use=verify_build_use(folder,package,evidence,bound)
                c.require(c.strict_json((folder/'corpus-manifest.json').read_bytes())==h.broad_contract(),'Broad manifest changed')
                for arm,row in v['arms'].items():
                    records=folder/arm/'records.jsonl';remote_records=PurePosixPath(package['build_workspace'])/run_name('broad')/arm/'records.jsonl'
                    lab={**h.s.SCOPE_ENV,h.s.BROAD_ENV:str(remote_records)}
                    verify_process(row,folder/arm,'execution',arm,'broad',['p1e2_full_state_corpus','--exact','--test-threads=1','--show-output'],lab,package,evidence,use,'complete')
                    proof=h.s.old_ci.named_test_proof((folder/arm/'execution.stdout').read_text(encoding='utf8'),['p1e2_full_state_corpus'],1)
                    c.require(proof==row['test_proof'] and c.sha(records)==row['records_sha256'] and records.stat().st_size==row['records_bytes'],'Broad proof/records changed')
                    c.require(h.compare_broad.compare_records(records,records,h.broad_contract())==row['record_validation'],'Broad self validation changed')
                recomputed={a:h.compare_broad.compare_records(folder/'reference/records.jsonl',folder/a/'records.jsonl',h.broad_contract()) for a in ('off','on')}
                c.require({('reference_vs_'+a):p for a,p in recomputed.items()}==v['comparisons'],'Broad comparison differs')
                value.update(broad104_complete=True,broad_recomputed=recomputed,broad_fresh_named_tests=3)
            else:value['issues'].append({'broad':'incomplete','status':v['status']})
        else:value['issues'].append({'broad':'missing artifact/ledger'})
        value['status']='passed' if value['full500_complete'] and value['broad104_complete'] and not value['issues'] else 'incomplete'
        return 0 if value['status']=='passed' else 1
    except Exception as error:value.update(status='failed_validation',full500_complete=False,broad104_complete=False,error=f'{type(error).__name__}: {error}');return 1
    finally:
        value['scope']='All500 frozen B17 Full decisions and104 separate fallback cases. Full joint payloads retained. Fresh source-specific accuracy gate only; no global mechanics proof, original-a4 oracle repair or automatic adoption.'
        base_ci.write(path,value);print('::notice::P7h accuracy '+value['status']+'; full500='+str(value['full500_complete'])+'; broad104='+str(value['broad104_complete']))

def main():
    parser=argparse.ArgumentParser();parser.add_argument('stage',choices=('build','full500','broad','aggregate'));parser.add_argument('--workspace',type=Path,required=True);parser.add_argument('--shard',type=int);parser.add_argument('--package-sha')
    args=parser.parse_args();workspace=args.workspace.resolve()
    if args.stage=='build':build.build(workspace);return
    if args.stage=='full500':code=full500(workspace,args.shard,args.package_sha)
    elif args.stage=='broad':code=broad(workspace,args.package_sha)
    else:code=aggregate(workspace,args.package_sha)
    raise SystemExit(code)
if __name__=='__main__':main()
