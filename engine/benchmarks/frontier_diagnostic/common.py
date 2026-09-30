"""P17 binding and exact output contracts, sharing the frozen distribution protocol."""
from pathlib import Path
import copy
import importlib.util
import json
import re
import sys
HERE=Path(__file__).resolve().parent
INHERITED=HERE.parent/'turn_distribution'
sys.path.append(str(INHERITED))
import contract as c
spec=importlib.util.spec_from_file_location('inherited_distribution_ci',INHERITED/'ci.py')
base_ci=importlib.util.module_from_spec(spec);spec.loader.exec_module(base_ci)
SOURCE_PARENT='a4a881420ca35335c6e0ac4c73fd9e686dd617c0'
FEATURE='experiment-frontier-observer'
ENVIRONMENT={'LAB_ENGINE_FRONTIER_OBSERVER':'1'}
BENCHMARK='engine/scenario/src/bin/lab-distribution-bench.rs'
BENCHMARK_SHA='8b04912f6494a3617d8956299ae6cfb9191221d46027e6d2c46df72c9d588e1a'
PLAN_SHA={'opening-0000':'bbd7c02f87855ab7d0790cd4146c1eeee8885ae00ca5da5ef26a9cf14559ea0f',
    'opening-0429':'2a6ec43ac6e821e1d710a94542f80f6ab04f37416406fb6a932b40f3d6de89ec'}

def binding():
    value=c.strict_json((HERE/'source-binding.json').read_bytes())
    c.require(re.fullmatch('[0-9a-f]{40}',value['source_sha']) is not None,'P17 source is not frozen')
    c.require(value['source_parent']==SOURCE_PARENT and value['benchmark_sha256']==BENCHMARK_SHA,'Wrong P17 parent or inherited benchmark')
    c.require(value['observer_feature']==FEATURE and value['observer_environment']==ENVIRONMENT,'Observer contract changed')
    pins=value['changed_file_sha256']
    c.require(isinstance(pins,dict) and pins and all(name.startswith('engine/core/') and '..' not in name.split('/')
        and re.fullmatch('[0-9a-f]{64}',digest) for name,digest in pins.items()),'Source delta is unbound or outside core observer scope')
    c.require(value['required_observer_tests'] and all(isinstance(name,str) and name for name in value['required_observer_tests']),
        'Observer named-test contract is unbound')
    c.require(isinstance(value['observer_schema'],dict) and value['observer_schema'],'Observer schema is unbound')
    c.require(value['observer_schema']==c.strict_json((HERE/'observer-contract.json').read_bytes()),'Frozen observer contract differs')
    c.require(set(pins)==set(value['observer_schema']['source_allowlist']),'Unexpected source file in observer delta')
    c.require('UNBOUND' not in value['observer_test_filter'],'Observer test filter is unbound')
    return value

def features(observer):return list(c.CORE_FEATURES)+([FEATURE] if observer else [])
def feature_args(observer):return ['--features',','.join('lab-engine/'+v for v in features(observer))]
def fixed_case(workspace,case_id):
    c.require(case_id in PLAN_SHA,'Unapproved diagnostic case')
    manifest=c.corpus(workspace/'controller');case=manifest['cases'][int(case_id.rsplit('-',1)[1])]
    c.require(case['id']==case_id,'Case order differs')
    path=c.safe_file(workspace/'controller','engine/benchmarks/frontier_diagnostic/'+case_id+'.plan.json')
    c.require(c.sha(path)==PLAN_SHA[case_id],'Original opening plan changed');c.description(c.line(path),case)
    return case,path

def semantic_result(value,plan,case):
    c.result(value,plan,case);out=copy.deepcopy(value)
    for key in ('kernel_ns','metric_prepare_ns'):del out['reference'][key]
    for sample in out['samples']:
        for key in ('kernel_ns','metric_ns'):del sample[key]
    return out
