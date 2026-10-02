"""One fresh build/test job; immutable binary package consumed by all accuracy jobs."""
from pathlib import Path
import json,os,re,shutil,subprocess
import suffix_contract as h
from suffix_contract import c,base_ci
PACKAGE_SCHEMA=1

def build(workspace):
    root=h.package_root(workspace);root.mkdir(exist_ok=False);path=root/'build-receipt.json'
    value={'status':'building','commands':[],'arms':{},'cached_regression_reused':False}
    try:
        bound=h.binding();sources=h.source_fingerprint(workspace,bound);h.corpus(workspace)
        rustc=base_ci.command(['rustc','-Vv'],workspace);c.require(rustc.startswith('rustc 1.98.1 '),'Toolchain differs')
        value.update(source_proof=sources,source_sha=bound['source_sha'],reference_sha=h.REFERENCE,source_binding_sha256=c.sha(h.HERE/'source-binding.json'),controller_sha=base_ci.command(['git','rev-parse','HEAD'],workspace/'controller'),rustc=rustc,build_workspace=str(workspace),run_id=os.environ['GITHUB_RUN_ID'],run_attempt=os.environ['GITHUB_RUN_ATTEMPT'])
        for arm in h.BUILD_ARMS:
            env=h.environment(workspace,arm);c.require(not any((h.target(workspace,arm)/'release/.fingerprint').glob('lab-*')),'Cached workspace test/build forbidden')
            for label,argv,names in h.suites(bound,arm):
                log=root/h.log_name(arm,label);command_env=dict(env)
                if label=='broad_contract':command_env[h.s.BROAD_MANIFEST_ENV]=(root/(arm+'-corpus-manifest.json')).as_posix()
                row={'arm':arm,'label':label,'argv':argv,'log':log.name,'lab_environment':{k:v for k,v in command_env.items() if k.startswith('LAB_')}}
                value['commands'].append(row);base_ci.write(path,value)
                with log.open('xb') as stream:process=subprocess.run(argv,cwd=h.source_root(workspace,arm)/'engine',env=command_env,stdout=stream,stderr=subprocess.STDOUT,timeout=600)
                row.update(returncode=process.returncode,log_sha256=c.sha(log));c.require(process.returncode==0,'Fresh test/build failed')
                if names:row['test_proof']=h.proof(label,log.read_text(encoding='utf8'),names)
                if label=='broad_contract':
                    manifest=root/(arm+'-corpus-manifest.json');c.require(c.strict_json(manifest.read_bytes())==h.broad_contract(),'Broad manifest differs')
                    row['manifest_sha256']=c.sha(manifest)
            proofs=h.fingerprints(workspace,arm);bins={}
            if arm!='edge':
                candidates=[p for p in (h.target(workspace,arm)/'release/deps').glob(h.BROAD+'-*') if p.is_file() and p.suffix=='']
                c.require(len(candidates)==1,'Missing/ambiguous broad executable')
                sources={'joint':h.target(workspace,arm)/'release'/h.TARGET,'broad':candidates[0]}
                if arm!='reference':sources['benchmark']=h.target(workspace,arm)/'release'/h.BENCHMARK
                for kind,binary in sources.items():
                    c.require(binary.is_file() and not binary.is_symlink(),'Missing executable')
                    dest=h.package_bin(workspace,arm,kind);dest.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(binary,dest)
                    bins[kind]={'relative_path':dest.relative_to(root).as_posix(),'sha256':c.sha(dest),'built_path':str(binary)}
            for p in proofs:
                src=h.target(workspace,arm)/'release/.fingerprint'/p['path'];dest=root/'fingerprints'/arm/p['path'];dest.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(src,dest)
            value['arms'][arm]={'source_sha':h.source_sha(bound,arm),'features':h.features(arm),'binaries':bins,'compiler_features':proofs}
        value.update(status='success',fresh_named_test_executions=h.named_count(bound))
    except Exception as error:value.update(status='failed',error=f'{type(error).__name__}: {error}');raise
    finally:base_ci.write(path,value)
    verify_evidence(root,bound,str(workspace))
    inventory={p.relative_to(root).as_posix():c.sha(p) for p in sorted(root.rglob('*')) if p.is_file()}
    package={'schema':PACKAGE_SCHEMA,'controller_sha':value['controller_sha'],'source_sha':bound['source_sha'],'reference_sha':h.REFERENCE,'source_binding_sha256':value['source_binding_sha256'],'run_id':value['run_id'],'run_attempt':value['run_attempt'],'build_workspace':str(workspace),'file_sha256':inventory}
    base_ci.write(root/'package.json',package)
    digest=c.sha(root/'package.json')
    with Path(os.environ['GITHUB_OUTPUT']).open('a',encoding='utf8') as out:out.write('package_sha256='+digest+'\n')
    return package

def verify_evidence(root,bound,workspace):
    value=c.strict_json((root/'build-receipt.json').read_bytes())
    c.require(value['status']=='success' and value['cached_regression_reused'] is False and value['source_sha']==bound['source_sha'] and value['reference_sha']==h.REFERENCE and value['source_binding_sha256']==c.sha(h.HERE/'source-binding.json'),'Missing/mixed build evidence')
    c.require(value['build_workspace']==workspace and value['fresh_named_test_executions']==h.named_count(bound),'Build workspace/test count differs')
    c.require(value['source_proof']==h.expected_source_proof(bound),'Source proof differs')
    c.require(value['rustc'].startswith('rustc 1.98.1 '),'Compiler differs')
    expected=[(arm,*row) for arm in h.BUILD_ARMS for row in h.suites(bound,arm)]
    c.require(len(value['commands'])==len(expected) and set(value['arms'])==set(h.BUILD_ARMS),'Incomplete fresh build')
    count=0
    for row,(arm,label,argv,names) in zip(value['commands'],expected):
        log=c.safe_file(root,h.log_name(arm,label))
        env={h.s.BROAD_MANIFEST_ENV:Path(workspace).as_posix()+'/'+h.PACKAGE+'/'+arm+'-corpus-manifest.json'} if label=='broad_contract' else {}
        c.require((row['arm'],row['label'],row['argv'],row['returncode'],row['lab_environment'])==(arm,label,argv,0,env),'Fresh command/environment differs')
        c.require(row['log']==log.name and row['log_sha256']==c.sha(log),'Fresh log differs')
        if names:
            proof=h.proof(label,log.read_text(encoding='utf8'),names);c.require(row['test_proof']==proof,'Named test proof differs');count+=proof['passed']
        if label=='broad_contract':
            manifest=c.safe_file(root,arm+'-corpus-manifest.json');c.require(row['manifest_sha256']==c.sha(manifest) and c.strict_json(manifest.read_bytes())==h.broad_contract(),'Broad manifest changed')
    c.require(count==h.named_count(bound),'Missing fresh named tests')
    for arm,row in value['arms'].items():
        c.require(row['source_sha']==h.source_sha(bound,arm) and row['features']==h.features(arm),'Compiled arm identity differs')
        found={k:set() for k in h.fingerprint_names(arm)};seen=set()
        for proof in row['compiler_features']:
            path=c.safe_file(root/'fingerprints'/arm,proof['path']);data=c.strict_json(path.read_bytes())
            c.require(proof['path'] not in seen and data==proof['content'] and c.sha(path)==proof['sha256'],'Compiler fingerprint changed/duplicated');seen.add(proof['path'])
            package=next((p for p in found if proof['path'].startswith(p+'-')),None)
            c.require(package is not None and path.name in h.fingerprint_names(arm)[package],'Wrong compiler target')
            actual=data['features'];actual=c.strict_json(actual) if isinstance(actual,str) else actual
            wanted=set(h.features(arm)) if package=='lab-engine' else set()
            c.require(isinstance(actual,list) and len(actual)==len(set(actual)) and set(actual)==wanted and proof['features']==actual and data['rustflags']==['-Ctarget-cpu=x86-64'],'Actual compiler configuration differs')
            found[package].add(path.name)
        c.require(found==h.fingerprint_names(arm),'Missing compiler target')
        c.require(set(row['binaries'])==set(h.bin_kinds(arm)),'Missing packaged executable')
        for kind,entry in row['binaries'].items():
            expected='bin/'+arm+'/'+{'joint':'joint-export','broad':'broad-tests','benchmark':'benchmark'}[kind]
            c.require(entry['relative_path']==expected and c.sha(c.safe_file(root,expected))==entry['sha256'],'Executable bytes differ')
    return value

def use_package(workspace,expected_sha,make_executable=True):
    root=h.package_root(workspace);c.require(re.fullmatch('[0-9a-f]{64}',expected_sha) and c.sha(root/'package.json')==expected_sha,'Wrong build package digest')
    package=c.strict_json((root/'package.json').read_bytes());bound=h.binding()
    c.require(package['schema']==PACKAGE_SCHEMA and package['source_sha']==bound['source_sha'] and package['reference_sha']==h.REFERENCE and package['source_binding_sha256']==c.sha(h.HERE/'source-binding.json'),'Package source/schema differs')
    c.require(package['controller_sha']==base_ci.command(['git','rev-parse','HEAD'],workspace/'controller'),'Package controller differs')
    c.require(package['run_id']==os.environ['GITHUB_RUN_ID'] and package['run_attempt']==os.environ['GITHUB_RUN_ATTEMPT']=='1','Package belongs to another run/attempt')
    c.require(package['build_workspace']==str(workspace),'Embedded fixture paths require same runner workspace')
    c.require(not root.is_symlink() and not any(p.is_symlink() for p in root.rglob('*')),'Symlink package member forbidden')
    actual={p.relative_to(root).as_posix() for p in root.rglob('*') if p.is_file() and p.relative_to(root).as_posix()!='package.json'}
    c.require(actual==set(package['file_sha256']),'Missing/extra package members')
    for name,digest in package['file_sha256'].items():c.require(c.sha(c.safe_file(root,name))==digest,'Package member changed')
    evidence=verify_evidence(root,bound,str(workspace))
    if make_executable:
        for arm in h.ARMS:
            for kind in h.bin_kinds(arm):
                path=h.package_bin(workspace,arm,kind);path.chmod(0o755)
                c.require(c.sha(path)==evidence['arms'][arm]['binaries'][kind]['sha256'],'Permission restoration altered binary')
    return package,evidence
