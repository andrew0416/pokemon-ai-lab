"""Bounded P12 correctness runner; no benchmarks. --plan never invokes Cargo.

CI-only adaptation of the source implementation's bounded runner. Dependencies
must be provisioned beforehand; Cargo remains locked, offline, jobs=1, opt0.
"""
from pathlib import Path
import argparse
import hashlib
import json
import os
import re
import subprocess

RUNTIME = 'experiment-volatile-hash-update'
OBSERVER = RUNTIME + '-observer'
COMPACT = 'experiment-compact-volatiles'
COMMON = ['experiment-hurt-readers','experiment-prepared-turn','experiment-leaf-ending-states',
          COMPACT,'experiment-replay-action-keys']
ARMS = {
    'dense-off': [], 'dense-on': [RUNTIME], 'dense-observer': [RUNTIME,OBSERVER],
    'compact-off': [COMPACT], 'compact-on': [COMPACT,RUNTIME],
    'compact-observer': [COMPACT,RUNTIME,OBSERVER],
    'common5-off': COMMON, 'common5-on': COMMON+[RUNTIME],
    'common5-observer': COMMON+[RUNTIME,OBSERVER],
}
CORE_TESTS = ['hashed_writes_and_reverse_match_original_for_singles_and_doubles',
              'mismatched_instruction_old_is_ignored_on_apply_and_used_on_reverse',
              'other_instruction_hash_paths_keep_original_behavior']
SCENARIO_TESTS = ['bounded_turn_records_preserve_exact_outputs_and_rollback',
                  'errors_and_seeded_samples_have_exact_records_and_restore_inputs']
UNIT_TESTS = ['volatile::hash_update_tests::replace_returns_actual_old_for_every_cell_and_transition',
              'volatile::hash_update_tests::replace_preserves_inactive_fields_boundaries_spill_and_clone_ownership']

def digest(path): return hashlib.sha256(path.read_bytes()).hexdigest()

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--engine',type=Path,default=Path(__file__).resolve().parent/'source/engine')
    parser.add_argument('--output',type=Path)
    parser.add_argument('--target',type=Path)
    parser.add_argument('--plan',action='store_true')
    args=parser.parse_args()
    if args.plan:
        print(json.dumps({'arms':ARMS,'core_integration_tests':CORE_TESTS,
            'scenario_tests':SCENARIO_TESTS,'on_unit_tests':UNIT_TESTS,
            'observer_test':'observer_proves_one_location_lookup_and_expected_rank_counts',
            'compact_on_unit_test':'volatile::compact::tests::replace_keeps_set_registry_capacity_and_spill_representation',
            'separate_output_pairs':['dense','compact','common5'],
            'performance_measured':False,'ci_only':True},indent=2))
        return
    if os.environ.get('GITHUB_ACTIONS')!='true': parser.error('CI-only execution; use --plan locally')
    selected=list(ARMS)
    if args.output is None or args.target is None: parser.error('--output and --target are required')
    output=args.output.resolve();output.mkdir(parents=True,exist_ok=False)
    if args.target.exists(): parser.error('P12 logic target must be fresh')
    engine=args.engine.resolve()
    env=dict(os.environ)
    env.update(CARGO_BUILD_JOBS='1',CARGO_PROFILE_DEV_OPT_LEVEL='0',
             CARGO_PROFILE_TEST_OPT_LEVEL='0',CARGO_PROFILE_DEV_DEBUG='0',CARGO_PROFILE_TEST_DEBUG='0',
             RUSTFLAGS='-Ctarget-cpu=x86-64',RUST_MIN_STACK='16777216',RUST_TEST_THREADS='1',
             RAYON_NUM_THREADS='1',LAB_ENGINE_FACTORED='0',CARGO_TARGET_DIR=str(args.target.resolve()))
    env.pop('LAB_ENGINE_STATS',None);env.pop('LAB_P12_RECORDS',None)
    env.pop('CARGO_ENCODED_RUSTFLAGS',None)
    pinned=json.loads((Path(__file__).resolve().parent/'p12-source.json').read_text())['file_sha256']
    def verify_source():
        assert {name:digest(engine.parent/name) for name in pinned}==pinned,'Source changed during checks'
    verify_source()
    receipt={'passed':False,'performance_measured':False,'commands':[],'compiled_features':{},'artifacts':{},'comparisons':[],
             'source_files':pinned,
             'ci_only':True,'selected_arms':selected,'full_ci_matrix':True}
    def save(): (output/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
    def run(name,command,extra=None):
        print(name,flush=True)
        log=output/(name+'.log')
        result=subprocess.run(command,cwd=engine,env=env|dict(extra or {}),stdout=subprocess.PIPE,
                              stderr=subprocess.STDOUT,text=True,encoding='utf-8',errors='replace',timeout=900)
        log.write_text(result.stdout,encoding='utf-8')
        receipt['commands'].append({'name':name,'command':command,'exit_code':result.returncode,
                                    'log':log.name,'sha256':digest(log)})
        save()
        if result.returncode: raise RuntimeError('Failed: '+name+'; see '+str(log))
        return result.stdout
    def flags(package,features):
        names=[('lab-engine/'+f) if package=='lab-scenario' else f for f in features]
        if package=='lab-scenario' and OBSERVER in features:
            names.append('lab-scenario/'+OBSERVER)
        return ['--features',','.join(names)] if names else []
    def assert_tests(text,names,exact=False):
        found=re.findall(r'^test (\S+) \.\.\. ok$',text,re.M)
        for name in names: assert found.count(name)==1,(name,found)
        if exact: assert set(found)==set(names),(found,names)
        assert re.search(r'test result: ok\. \d+ passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;',text)
    for arm in selected:
        features=ARMS[arm]
        unit=run(arm+'-units',['cargo','test','--locked','--offline','-j1','-p','lab-engine','--lib']+
                 flags('lab-engine',features)+['--','--test-threads=1'])
        required=list(UNIT_TESTS) if RUNTIME in features else []
        if RUNTIME in features and COMPACT in features:
            required.append('volatile::compact::tests::replace_keeps_set_registry_capacity_and_spill_representation')
        assert_tests(unit,required)
        for package in ('lab-engine','lab-scenario'):
            build=run(arm+'-'+package+'-build',['cargo','test','--locked','--offline','-j1','-p',package,
                      '--test','volatile_hash_update','--no-run','--message-format=json']+flags(package,features))
            artifacts=[]
            for line in build.splitlines():
                try: event=json.loads(line)
                except ValueError: continue
                if event.get('reason')=='compiler-artifact': artifacts.append(event)
            core=[a for a in artifacts if a['target']['name']=='lab_engine']
            assert core and all(set(a['features'])==set(features) for a in core),(arm,core)
            assert not any('inline-runstart' in f or 'borrowed-child' in f for a in core for f in a['features'])
            receipt['compiled_features'][arm+'/'+package]=core[-1]['features']
            executable=[a['executable'] for a in artifacts if a['target']['name']=='volatile_hash_update' and a.get('executable')]
            assert len(executable)==1,executable
            receipt['artifacts'][arm+'/'+package]={'executable':executable[0],
                'sha256':digest(Path(executable[0])),'core_features':core[-1]['features']}
            extra={}
            if package=='lab-scenario':
                records=output/(arm+'-records');records.mkdir()
                extra['LAB_P12_RECORDS']=str(records)
            text=run(arm+'-'+package+'-tests',[executable[0],'--nocapture','--test-threads=1'],extra)
            names=list(SCENARIO_TESTS if package=='lab-scenario' else CORE_TESTS)
            if package=='lab-engine' and OBSERVER in features:
                names.append('observer_proves_one_location_lookup_and_expected_rank_counts')
            assert_tests(text,names,exact=True)
    for group in ('dense','compact','common5'):
        if group+'-off' not in selected or group+'-on' not in selected: continue
        arms=[group+'-off',group+'-on']
        if group+'-observer' in selected: arms.append(group+'-observer')
        for filename in ('turns.jsonl','errors-samples.jsonl'):
            paths=[output/(arm+'-records')/filename for arm in arms]
            raw=[p.read_bytes() for p in paths]
            assert raw[0] and all(b==raw[0] for b in raw[1:]),(group,filename)
            receipt['comparisons'].append({'group':group,'file':filename,'arms':arms,
                'bytes':len(raw[0]),'records':len(raw[0].splitlines()),'sha256':digest(paths[0])})
    verify_source()
    receipt['passed']=True;save()
    print(json.dumps({'passed':True,'arms':len(selected),'exact_stream_comparisons':len(receipt['comparisons'])}))

if __name__=='__main__': main()
