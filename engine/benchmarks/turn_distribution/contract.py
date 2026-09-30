"""Pinned internal R1 turn-distribution protocol; not a reproduction of paper numbers."""
import hashlib
import json
import math
from pathlib import Path,PurePosixPath
import re

CANONICAL_SOURCE='5d2d3581150b4464cc6586fd0c818be4b4a15d46'
SOURCE_SHA='a4a881420ca35335c6e0ac4c73fd9e686dd617c0'
SOURCE_FILES={'engine/scenario/src/bin/lab-distribution-bench.rs': '8b04912f6494a3617d8956299ae6cfb9191221d46027e6d2c46df72c9d588e1a'}
CORPUS_PATH='engine/benchmarks/turn_distribution/corpus/manifest.json'
CORPUS_SHA='16410bfd800213a7af9f9fd896d5a8e4fb7da39b0a0912192263c1a7625272ec'
COUNT=16
SAMPLE_COUNTS=(16,64,256)
CORE_FEATURES=tuple('experiment-'+name for name in ('hurt-readers','prepared-turn','leaf-ending-states',
    'compact-volatiles','replay-action-keys','borrowed-child-keys'))
FEATURE_ARGS=['--features',','.join('lab-engine/'+name for name in CORE_FEATURES)]
TESTS=tuple('tests::'+name for name in (
    'seeded_selection_is_order_independent_and_preserves_mega_move_variants',
    'non_hp_projection_keeps_all_other_fields_and_hidden_party_order',
    'overlapping_factored_components_match_expansion_and_exact_tv',
    'correlated_components_and_fixed_hp_singletons_do_not_gain_cross_support',
    'frozen_plan_rejects_changed_state_actions_seed_and_noncanonical_bytes',
    'invalid_probability_supports_fail_and_top32_is_projection_posthoc',
    'bounded_real_full_factored_and_sample_paths_preserve_state_and_suspension'))
EPS=1e-9

def require(ok,message):
    if not ok:raise ValueError(message)
def sha(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()
def strict_json(raw):
    def pairs(items):
        result={}
        for key,value in items:
            require(key not in result,'Duplicate JSON key');result[key]=value
        return result
    def reject(value):raise ValueError('Nonfinite JSON constant '+value)
    return json.loads(raw,object_pairs_hook=pairs,parse_constant=reject)
def uint(value,bits=64):return type(value) is int and 0<=value<2**bits
def number(value):return type(value) in (int,float) and math.isfinite(value)
def probability(value):return number(value) and 0<=value<=1
def same(a,b):return number(a) and number(b) and abs(a-b)<=EPS
def fields(value,names):require(isinstance(value,dict) and set(value)==set(names.split()),'Unexpected record fields')
def line(path):
    raw=Path(path).read_bytes()
    require(raw.endswith(b'\n') and b'\r' not in raw and len(raw.splitlines())==1,'Exactly one complete LF JSON line required')
    return strict_json(raw)
def safe_file(root,name):
    p=PurePosixPath(name)
    require(not p.is_absolute() and '\\' not in name and '..' not in p.parts and ':' not in name,'Unsafe relative input path')
    path=root.joinpath(*p.parts)
    require(path.is_file() and not path.is_symlink() and path.resolve().is_relative_to(root.resolve()),'Input missing or outside frozen controller')
    return path

def corpus(root):
    path=safe_file(root,CORPUS_PATH);require(sha(path)==CORPUS_SHA,'Frozen corpus manifest changed')
    doc=strict_json(path.read_bytes())
    require(doc['schema']==1 and doc['canonical_source_sha']==CANONICAL_SOURCE and doc['count']==500,'Wrong corpus identity')
    cases=doc['cases'];require(isinstance(cases,list) and len(cases)==500,'Exactly 500 frozen cases required')
    for index,case in enumerate(cases):
        fields(case,'id scenario scenario_sha256 joint_seed sample_seeds teams orders')
        require(case['id']==f'opening-{index:04d}','Frozen order or case identity changed')
        require(case['scenario']==str(PurePosixPath(CORPUS_PATH).parent/'scenarios'/(case['id']+'.json')),'Scenario path mismatch')
        require(sha(safe_file(root,case['scenario']))==case['scenario_sha256'],'Frozen scenario bytes changed')
        require(uint(case['joint_seed']) and isinstance(case['sample_seeds'],list) and len(case['sample_seeds'])==5
                and all(uint(s) for s in case['sample_seeds']) and len(set(case['sample_seeds']))==5,'Invalid frozen seeds')
    for team in doc['provenance']:
        for name,digest in team['files'].items():
            path=str(PurePosixPath(CORPUS_PATH).parent/'provenance'/team['id']/name)
            require(sha(safe_file(root,path))==digest,'Frozen team provenance changed')
    return doc

DESCRIPTION_FIELDS='schema kind joint_seed rng position_order joint_policy ruleset position_count position_index position_probability position_probability_bits position_total_mass eligible_joint_counts joint_indices full_state_debug party_order_debug choices_debug choices'
def description(value,case):
    fields(value,DESCRIPTION_FIELDS)
    require(type(value['schema']) is int and value['schema']==1 and value['kind']=='frozen-opening-selection'
            and value['joint_seed']==case['joint_seed'] and uint(value['joint_seed']) and value['ruleset']=='CHAMPIONS_MC','Description identity differs')
    require(uint(value['position_count']) and value['position_count']>0 and uint(value['position_index'])
            and value['position_index']<value['position_count'],'Invalid selected opening position')
    require(probability(value['position_probability']) and value['position_probability']>0
            and uint(value['position_probability_bits']) and same(value['position_total_mass'],1),'Invalid opening mass')
    for key in ('rng','position_order','joint_policy','full_state_debug','party_order_debug','choices_debug'):
        require(isinstance(value[key],str) and value[key],'Missing description '+key)
    counts,indices=value['eligible_joint_counts'],value['joint_indices']
    require(isinstance(counts,list) and len(counts)==2 and all(uint(n) and n>0 for n in counts)
            and isinstance(indices,list) and len(indices)==2 and all(uint(i) and i<n for i,n in zip(indices,counts)),
            'Invalid joint choice identity')
    require(isinstance(value['choices'],list) and len(value['choices'])==2,'Missing joint choices')
    for side in value['choices']:
        require(isinstance(side,list) and len(side)==2,'Missing slot actions')
        for action in side:
            fields(action,'move_index target gimmick')
            require(uint(action['move_index']) and action['move_index']<4 and type(action['target']) is int
                    and action['gimmick'] in ('None','Mega'),'Unexpected action/gimmick')
    return value

def metric(value):
    fields(value,'coverage tv outside_reference_mass unique_states')
    require(all(probability(value[k]) for k in ('coverage','tv','outside_reference_mass'))
            and uint(value['unique_states']) and value['unique_states']>0,'Invalid metric bounds')
    require(value['tv']+EPS>=1-value['coverage'] and value['outside_reference_mass']<=value['tv']+EPS,'Inconsistent TV/coverage')
    require(value['outside_reference_mass']==0,'Sample contains an outcome outside the exact reference')

def result(value,plan,case):
    fields(value,'schema status description metric_schema reference non_hp_state_posthoc_top32 samples all_state_restored all_sample_outcomes_in_reference timing_scope suspension_scope sample_policy probability_policy execution_policy')
    require(type(value['schema']) is int and value['schema']==1 and value['status']=='ok','Benchmark did not complete')
    description(value['description'],case);require(value['description']==plan,'Measured plan differs from frozen description')
    require(value['all_state_restored'] is True and value['all_sample_outcomes_in_reference'] is True,'State/reference correctness failed')
    for key in ('metric_schema','timing_scope','suspension_scope','sample_policy','probability_policy','execution_policy'):require(isinstance(value[key],str) and value[key],'Missing measurement scope')
    ref=value['reference'];fields(ref,'method kernel_ns metric_prepare_ns components total_mass tv_bound flat_count_upper_bound full_support_materialized suspended_components')
    require(ref['method']=='factored-full-exact' and ref['tv_bound']==0 and type(ref['tv_bound']) in (int,float)
            and ref['full_support_materialized'] is False,'Reference is not uncapped exact factored Full')
    require(uint(ref['kernel_ns']) and ref['kernel_ns']>0 and uint(ref['metric_prepare_ns'])
            and uint(ref['components']) and ref['components']>0 and same(ref['total_mass'],1)
            and number(ref['flat_count_upper_bound']) and ref['flat_count_upper_bound']>=ref['components']
            and uint(ref['suspended_components']) and ref['suspended_components']<=ref['components'],'Invalid reference accounting')
    top=value['non_hp_state_posthoc_top32'];fields(top,'projection support_size retained_states retained_mass omitted_mass renormalized_tv method timing_speedup_claim')
    require(top['projection']=='non_hp_state' and top['timing_speedup_claim'] is False and isinstance(top['method'],str)
            and uint(top['support_size']) and 0<top['support_size']<=ref['components']
            and top['retained_states']==min(top['support_size'],32),'Invalid top32 projection scope')
    require(all(probability(top[k]) for k in ('retained_mass','omitted_mass','renormalized_tv'))
            and same(top['retained_mass']+top['omitted_mass'],1) and same(top['omitted_mass'],top['renormalized_tv']),
            'Invalid posthoc retained mass/TV')
    rows=value['samples'];expected=[(n,s) for n in SAMPLE_COUNTS for s in case['sample_seeds']]
    require(isinstance(rows,list) and len(rows)==15,'Incomplete sampling count/seed matrix')
    for sample,(count,seed) in zip(rows,expected):
        fields(sample,'count seed kernel_ns metric_ns raw_outcomes sample_total_mass unique_full_states full_state non_hp_state')
        require(uint(sample['count']) and sample['count']==count and uint(sample['seed']) and sample['seed']==seed,'Sample order/count/seed changed')
        require(uint(sample['kernel_ns']) and sample['kernel_ns']>0 and uint(sample['metric_ns'])
                and uint(sample['raw_outcomes']) and 0<sample['raw_outcomes']<=count
                and uint(sample['unique_full_states']) and 0<sample['unique_full_states']<=sample['raw_outcomes']
                and same(sample['sample_total_mass'],1),'Invalid sample timing/support/mass')
        full,projected=sample['full_state'],sample['non_hp_state'];metric(full);metric(projected)
        require(full['unique_states']==sample['unique_full_states'] and projected['unique_states']<=full['unique_states']
                and projected['coverage']+EPS>=full['coverage'] and projected['tv']<=full['tv']+EPS,'Invalid projection/full-state relationship')
    return value
