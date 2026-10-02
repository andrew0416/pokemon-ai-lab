"""Source-specific global accuracy gate. Timing cannot consume earlier-candidate evidence."""
from pathlib import Path
import os
import suffix_contract as h
from suffix_contract import c,base_ci
DIRECTORY='p7h-accuracy-gate'
def issue(workspace,digest):
    import suffix_accuracy as accuracy
    import suffix_build as build
    result=accuracy.aggregate(workspace,digest)
    if result:return result
    root=workspace/'p7h-aggregate-results';summary=c.strict_json((root/'summary.json').read_bytes())
    package,evidence=build.use_package(workspace,digest,False)
    c.require(summary['status']=='passed' and summary['full500_complete'] and summary['broad104_complete'] and summary['verified_full500_cases']==500 and summary['broad_fresh_named_tests']==3 and not summary['issues'],'Incomplete global accuracy')
    value={'schema':1,'kind':'p7h-global-accuracy-gate','passed':True,'source_sha':h.candidate_sha(),'reference_sha':h.REFERENCE,
      'source_binding_sha256':c.sha(h.HERE/'source-binding.json'),'controller_sha':package['controller_sha'],'run_id':package['run_id'],'run_attempt':package['run_attempt'],
      'package_sha256':digest,'features':{arm:h.features(arm) for arm in h.ARMS},
      'binary_sha256':{arm:{kind:data['sha256'] for kind,data in evidence['arms'][arm]['binaries'].items()} for arm in h.ARMS},
      'full500_complete':True,'broad104_complete':True,'full500_cases':500,'broad_cases':104,
      'summary_sha256':c.sha(root/'summary.json'),'adoption_approved':False}
    base_ci.write(root/'gate.json',value)
    with Path(os.environ['GITHUB_OUTPUT']).open('a',encoding='utf8') as out:out.write('gate_sha256='+c.sha(root/'gate.json')+'\n')
    return 0
def verify(workspace,gate_digest,package,evidence,package_digest):
    root=workspace/DIRECTORY;path=root/'gate.json'
    c.require(isinstance(gate_digest,str) and len(gate_digest)==64 and c.sha(path)==gate_digest,'Missing/different global accuracy gate')
    gate=c.strict_json(path.read_bytes());summary=c.strict_json((root/'summary.json').read_bytes())
    expected={'schema':1,'kind':'p7h-global-accuracy-gate','passed':True,'source_sha':h.candidate_sha(),'reference_sha':h.REFERENCE,
      'source_binding_sha256':c.sha(h.HERE/'source-binding.json'),'controller_sha':package['controller_sha'],'run_id':package['run_id'],'run_attempt':package['run_attempt'],
      'package_sha256':package_digest,'features':{arm:h.features(arm) for arm in h.ARMS},
      'binary_sha256':{arm:{kind:data['sha256'] for kind,data in evidence['arms'][arm]['binaries'].items()} for arm in h.ARMS},
      'full500_complete':True,'broad104_complete':True,'full500_cases':500,'broad_cases':104,
      'summary_sha256':c.sha(root/'summary.json'),'adoption_approved':False}
    c.require(gate==expected and package['run_attempt']=='1','Accuracy gate belongs to another source/build/run')
    c.require(summary['status']=='passed' and summary['full500_complete'] and summary['broad104_complete'] and summary['verified_full500_cases']==500 and summary['broad_fresh_named_tests']==3 and not summary['issues'],'Incomplete accuracy summary')
    c.require(summary['source_sha']==h.candidate_sha() and summary['reference_sha']==h.REFERENCE and summary['package_sha256']==package_digest and summary['controller_sha']==package['controller_sha'] and summary['fresh_build_named_tests']==h.named_count(h.binding()),'Accuracy summary identity differs')
    c.require(len(summary['shards'])==4 and [r['index'] for r in summary['shards']]==list(range(4)) and all(r['status']=='passed' and r['completed_cases']==125 for r in summary['shards']),'Missing completed accuracy shard')
    return gate
