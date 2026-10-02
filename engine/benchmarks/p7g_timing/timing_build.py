"""Build two binaries once; exact transferable package with 28 fresh named checks."""
from pathlib import Path
import os,re,shutil,subprocess
import timing_contract as h
from timing_contract import c,base_ci
def build(workspace):
    root=h.package_root(workspace);root.mkdir(exist_ok=False);path=root/'build-receipt.json'
    v={'status':'building','commands':[],'arms':{},'cached_regression_reused':False}
    try:
        b=h.binding();proof=h.source_proof(workspace,b);h.corpus(workspace)
        compiler=base_ci.command(['rustc','-Vv'],workspace);c.require(compiler.startswith('rustc 1.98.1 '),'Frozen compiler differs')
        v.update(source_proof=proof,source_sha=h.SOURCE,controller_sha=base_ci.command(['git','rev-parse','HEAD'],workspace/'controller'),source_binding_sha256=c.sha(h.HERE/'source-binding.json'),rustc=compiler,build_workspace=str(workspace),run_id=os.environ['GITHUB_RUN_ID'],run_attempt=os.environ['GITHUB_RUN_ATTEMPT'])
        for arm in h.ARMS:
            env=h.environment(workspace,arm);c.require(not any((h.target(workspace,arm)/'release/.fingerprint').glob('lab-*')),'Cached workspace compilation forbidden')
            for label,argv,names in h.commands(arm):
                log=root/h.log_name(arm,label);row={'arm':arm,'label':label,'argv':argv,'log':log.name,'lab_environment':{k:v_ for k,v_ in env.items() if k.startswith('LAB_')}}
                v['commands'].append(row);base_ci.write(path,v)
                with log.open('xb') as output:process=subprocess.run(argv,cwd=workspace/'source/engine',env=env,stdout=output,stderr=subprocess.STDOUT,timeout=600)
                row.update(returncode=process.returncode,log_sha256=c.sha(log));c.require(process.returncode==0,'Fresh command failed')
                if names:row['test_proof']=h.named_proof(label,log.read_text(encoding='utf8'),names)
            fp=h.fingerprints(workspace,arm)
            for row in fp:
                dest=root/'fingerprints'/arm/row['path'];dest.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(h.target(workspace,arm)/'release/.fingerprint'/row['path'],dest)
            source=h.target(workspace,arm)/'release'/h.TARGET;dest=h.binary(workspace,arm);dest.parent.mkdir(parents=True,exist_ok=True)
            c.require(source.is_file() and not source.is_symlink(),'Missing compiled binary');shutil.copyfile(source,dest)
            v['arms'][arm]={'source_sha':h.SOURCE,'features':h.features(arm),'binary':{'relative_path':dest.relative_to(root).as_posix(),'sha256':c.sha(dest),'built_path':str(source)},'compiler_features':fp}
        v.update(status='success',fresh_named_test_executions=28)
    except Exception as error:v.update(status='failed',error=f'{type(error).__name__}: {error}');raise
    finally:base_ci.write(path,v)
    verify_evidence(root,str(workspace))
    package={k:v[k] for k in ('controller_sha','source_sha','source_binding_sha256','build_workspace','run_id','run_attempt')}
    package.update(schema=1,file_sha256={p.relative_to(root).as_posix():c.sha(p) for p in root.rglob('*') if p.is_file()})
    base_ci.write(root/'package.json',package)
    with Path(os.environ['GITHUB_OUTPUT']).open('a',encoding='utf8') as output:output.write('package_sha256='+c.sha(root/'package.json')+'\n')
    return package
def verify_evidence(root,remote_workspace):
    v=c.strict_json((root/'build-receipt.json').read_bytes());b=h.binding()
    c.require(v['status']=='success' and v['cached_regression_reused'] is False and v['source_sha']==h.SOURCE and v['source_binding_sha256']==c.sha(h.HERE/'source-binding.json') and v['build_workspace']==remote_workspace and v['fresh_named_test_executions']==28,'Wrong build evidence')
    c.require(v['source_proof']==h.expected_source_proof(b) and v['rustc'].startswith('rustc 1.98.1 '),'Source/compiler proof differs')
    expected=[(arm,*r) for arm in h.ARMS for r in h.commands(arm)];c.require(len(v['commands'])==len(expected) and set(v['arms'])==set(h.ARMS),'Incomplete build')
    count=0
    for row,(arm,label,argv,names) in zip(v['commands'],expected):
        log=c.safe_file(root,h.log_name(arm,label))
        c.require((row['arm'],row['label'],row['argv'],row['log'],row['returncode'],row['lab_environment'])==(arm,label,argv,log.name,0,{}),'Fresh command/log/environment differs')
        c.require(row['log_sha256']==c.sha(log),'Fresh log changed')
        if names:
            proof=h.named_proof(label,log.read_text(encoding='utf8'),names);c.require(row['test_proof']==proof,'Fresh named proof differs');count+=proof['passed']
    c.require(count==28,'Missing fresh named tests')
    for arm,row in v['arms'].items():
        c.require(row['source_sha']==h.SOURCE and row['features']==h.features(arm),'Compiled source/features differ')
        h.check_fingerprints(row['compiler_features'],root/'fingerprints'/arm,arm)
        binary=row['binary'];c.require(binary['relative_path']=='bin/'+arm+'/benchmark' and c.sha(c.safe_file(root,binary['relative_path']))==binary['sha256'],'Actual binary bytes differ')
    return v
def verify_package(root,digest,controller,run_id,attempt,remote_workspace=None):
    c.require(isinstance(digest,str) and re.fullmatch('[0-9a-f]{64}',digest) and c.sha(root/'package.json')==digest,'Wrong package digest')
    p=c.strict_json((root/'package.json').read_bytes())
    c.require(p['schema']==1 and p['source_sha']==h.SOURCE and p['source_binding_sha256']==c.sha(h.HERE/'source-binding.json') and p['controller_sha']==controller and p['run_id']==str(run_id) and p['run_attempt']==str(attempt)=='1','Mixed package provenance')
    if remote_workspace is not None:c.require(p['build_workspace']==remote_workspace,'Different embedded source workspace')
    c.require(not root.is_symlink() and not any(f.is_symlink() for f in root.rglob('*')),'Symlink package forbidden')
    actual={f.relative_to(root).as_posix() for f in root.rglob('*') if f.is_file() and f.relative_to(root).as_posix()!='package.json'}
    c.require(actual==set(p['file_sha256']),'Package inventory differs')
    for name,d in p['file_sha256'].items():c.require(c.sha(c.safe_file(root,name))==d,'Package member changed')
    evidence=verify_evidence(root,p['build_workspace'])
    c.require((evidence['controller_sha'],evidence['run_id'],evidence['run_attempt'])==(controller,str(run_id),'1'),'Build receipt provenance differs')
    return p,evidence
def use_package(workspace,digest,executable=True):
    p,e=verify_package(h.package_root(workspace),digest,base_ci.command(['git','rev-parse','HEAD'],workspace/'controller'),os.environ['GITHUB_RUN_ID'],os.environ['GITHUB_RUN_ATTEMPT'],str(workspace))
    if executable:
        for arm in h.ARMS:
            binary=h.binary(workspace,arm);binary.chmod(0o755);c.require(c.sha(binary)==e['arms'][arm]['binary']['sha256'],'chmod altered binary')
    return p,e
