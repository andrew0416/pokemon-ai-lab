"""Fresh real producer/receipt/verifier path, shared log naming."""
import subprocess
from pathlib import Path
import pilot_contract as h
from pilot_contract import c,base_ci

def prepare(workspace):
    folder=workspace/h.RESULTS;folder.mkdir(exist_ok=False);value={'status':'preparing','adoption_approved':False}
    try:
        b=h.binding();_,cases=h.pilot(workspace);sources=h.verify_sources(workspace,b)
        rustc=base_ci.command(['rustc','-Vv'],workspace);c.require(rustc.startswith('rustc 1.98.1 '),'Toolchain changed')
        value.update(status='prepared',sources=sources,controller_sha=base_ci.command(['git','rev-parse','HEAD'],workspace/'controller'),
          source_binding_sha256=c.sha(h.HERE/'source-binding.json'),rustc=rustc,lscpu=base_ci.command(['lscpu'],workspace),
          features={a:h.features(a) for a in h.BUILD_ARMS},pilot_cases=[x[0]['id'] for x in cases],
          limits={'child_seconds':h.CHILD_SECONDS,'rss_bytes':h.RSS,'phase_seconds':h.PHASE_SECONDS,'job_minutes':35},
          timing_metric='reference.kernel_ns only: one Full factored API call; sample/export/metric costs excluded',
          observer='cfg(test) counter assertions only; release has no observer feature',
          scope='Eight-case pilot exact reference563 versus new source OFF/ON; no all500/global proof or adoption')
    except Exception as error:value.update(status='failed',error=f'{type(error).__name__}: {error}');raise
    finally:base_ci.write(folder/'provenance.json',value)

def build(workspace):
    folder=workspace/h.RESULTS;path=folder/'build-receipt.json';c.require(not path.exists(),'Build already attempted')
    value={'status':'building','commands':[],'arms':{},'cached_regression_reused':False}
    try:
        b=h.binding();h.verify_sources(workspace,b)
        for arm in h.BUILD_ARMS:
            env=h.environment(workspace,arm)
            c.require(not any((h.target(workspace,arm)/'release/.fingerprint').glob('lab-*')),'Cached workspace crate forbidden')
            for label,argv,names in h.commands(arm,b):
                log=folder/h.command_log_name(arm,label)
                row={'arm':arm,'label':label,'argv':argv,'log':log.name,'lab_environment':{k:v for k,v in env.items() if k.startswith('LAB_')}}
                value['commands'].append(row);base_ci.write(path,value)
                with log.open('xb') as stream:
                    result=subprocess.run(argv,cwd=h.source_root(workspace,arm)/'engine',env=env,stdout=stream,stderr=subprocess.STDOUT,timeout=600)
                row.update(returncode=result.returncode,log_sha256=c.sha(log));c.require(result.returncode==0,'Fresh build/test failed')
                if names:row['test_proof']=h.test_proof(label,log.read_text(encoding='utf8'),names)
            bins={}
            for kind in h.bin_kinds(arm):
                binary=h.target(workspace,arm)/'release'/h.BINS[kind];c.require(binary.is_file() and not binary.is_symlink(),'Missing binary')
                bins[kind]={'path':str(binary),'sha256':c.sha(binary)}
            proofs=h.fingerprints(workspace,arm)
            for proof in proofs:
                src=h.target(workspace,arm)/'release/.fingerprint'/proof['path'];dst=folder/'fingerprints'/arm/proof['path']
                dst.parent.mkdir(parents=True,exist_ok=True)
                with dst.open('xb') as out:out.write(src.read_bytes())
            value['arms'][arm]={'source_sha':h.source_sha(b,arm),'features':h.features(arm),'binaries':bins,'compiler_features':proofs}
        value.update(status='success',source_sha=b['source_sha'],reference_sha=h.REFERENCE,source_binding_sha256=c.sha(h.HERE/'source-binding.json'),fresh_named_test_executions=h.expected_count(b))
    except Exception as error:value.update(status='failed',error=f'{type(error).__name__}: {error}');raise
    finally:base_ci.write(path,value)

def verify_build(workspace,b):
    folder=workspace/h.RESULTS;v=c.strict_json((folder/'build-receipt.json').read_bytes())
    c.require(v['status']=='success' and v['cached_regression_reused'] is False and v['source_sha']==b['source_sha'] and v['reference_sha']==h.REFERENCE and v['source_binding_sha256']==c.sha(h.HERE/'source-binding.json'),'Missing/mixed fresh build')
    expected=[(arm,*row) for arm in h.BUILD_ARMS for row in h.commands(arm,b)]
    c.require(len(v['commands'])==len(expected) and set(v['arms'])==set(h.BUILD_ARMS),'Missing arm/command')
    count=0
    for row,(arm,label,argv,names) in zip(v['commands'],expected):
        log=folder/h.command_log_name(arm,label)
        c.require((row['arm'],row['label'],row['argv'],row['returncode'],row['lab_environment'])==(arm,label,argv,0,{}),'Fresh command changed')
        c.require(row['log']==log.name and c.sha(log)==row['log_sha256'],'Fresh log changed')
        if names:
            proof=h.test_proof(label,log.read_text(encoding='utf8'),names)
            c.require(proof==row['test_proof'],'Named proof changed');count+=proof['passed']
    c.require(count==v['fresh_named_test_executions']==h.expected_count(b),'Fresh test count differs')
    for arm in h.BUILD_ARMS:
        row=v['arms'][arm]
        c.require(row['source_sha']==h.source_sha(b,arm) and row['features']==h.features(arm) and row['compiler_features']==h.fingerprints(workspace,arm),'Arm source/features changed')
        c.require(set(row['binaries'])==set(h.bin_kinds(arm)),'Wrong binary set')
        for kind,entry in row['binaries'].items():
            path=h.target(workspace,arm)/'release'/h.BINS[kind]
            c.require(entry['path']==str(path) and path.is_file() and not path.is_symlink() and entry['sha256']==c.sha(path),'Binary changed')
    return v
