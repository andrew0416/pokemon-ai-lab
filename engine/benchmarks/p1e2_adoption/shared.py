"""Reuse immutable P1e protocol helpers; bind P1e2 test-only source independently."""
from pathlib import Path
import re, sys, tomllib
HERE=Path(__file__).resolve().parent
P1E=HERE.parent/'lazy_ko_damage'
sys.path.insert(0,str(P1E))
import common as p1
import ci as old_ci
import compare_public as old_compare
c=p1.c
base_ci=p1.base_ci
P1E_SOURCE='a10adfd9aeaebe9624bd8e4bd804789ddecf6167'
ORIGINAL=p1.SOURCE_PARENT
ARMS=('original','off','on')
RESULTS='p1e2-accuracy-results'
BROAD_TARGET='lazy_ko_damage_expanded'
BROAD_TESTS=['p1e2_corpus_contract','p1e2_full_state_corpus']
BROAD_ENV='LAB_P1E2_PUBLIC_RECORDS'
BROAD_MANIFEST_ENV='LAB_P1E2_CORPUS_MANIFEST'
SCOPE_ENV={'LAB_P1E2_SCOPE':'expanded'}
PUBLIC='engine/scenario/tests/lazy_ko_damage.rs'
MANIFEST='engine/scenario/Cargo.toml'
BROAD_FILE='engine/scenario/tests/lazy_ko_damage_expanded.rs'
REGISTRATION='\n[[test]]\nname = "lazy_ko_damage_expanded"\npath = "tests/lazy_ko_damage_expanded.rs"\n'

def prior_binding():
    return p1.binding()

def binding():
    value=c.strict_json((HERE/'source-binding.json').read_bytes())
    c.require(re.fullmatch('[0-9a-f]{40}',value['source_sha']) is not None,'P1e2 source unbound')
    c.require(value['source_parent']==P1E_SOURCE and value['source_ref'].startswith('refs/heads/codex/'),'Wrong P1e2 source parent/ref')
    c.require(value['inherited_p1e_file_sha256']==prior_binding()['changed_file_sha256'],'Inherited P1e pins changed')
    c.require(value['parent_p1e_binding_sha256']==c.sha(P1E/'source-binding.json'),'Inherited P1e binding changed')
    pins=value['test_only_file_sha256']
    c.require(isinstance(pins,dict) and MANIFEST in pins and BROAD_FILE in pins,'Missing test-only delta')
    for name,digest in pins.items():
        c.require(name==MANIFEST or name.startswith('engine/scenario/tests/p1e2') or name==BROAD_FILE
                  or name=='engine/scenario/src/bin/lab-joint-export.rs','Out-of-scope P1e2 change')
        c.require('..' not in name.split('/') and re.fullmatch('[0-9a-f]{64}',digest),'Unbound test-only hash')
    c.require(value['broad_target']==BROAD_TARGET and value['broad_tests']==BROAD_TESTS
              and value['broad_record_environment']==BROAD_ENV and value['broad_scope_environment']==SCOPE_ENV,'Broad harness identity changed')
    for field in ('source_receipt_sha256','root_source_review_sha256','logic_receipt_sha256','broad_comparator_sha256'):
        c.require(re.fullmatch('[0-9a-f]{64}',value[field]) is not None,'Unbound review '+field)
    c.require(value['broad_comparator_sha256']==c.sha(HERE/'compare_broad.py'),'Broad comparator changed')
    contract=value['broad_record_contract']
    import compare_broad
    cases=compare_broad.validate_contract(contract)
    c.require(100<=len(cases)<=200 and contract['distinct_input_count']>=85,'Expanded contract count is not the reviewed broad corpus')
    c.require(value['broad_manifest_environment']==BROAD_MANIFEST_ENV,'Broad manifest environment changed')
    return value

def environment(workspace,arm):
    c.require(arm in ARMS,'Bad arm')
    env=base_ci.environment(workspace)
    env['CARGO_TARGET_DIR']=str(workspace/('target-p1e2-'+arm))
    return env

def source_root(workspace,arm): return workspace/('original' if arm=='original' else 'source')

def verify_source(root,bound):
    command=base_ci.command
    c.require(command(['git','rev-parse','HEAD'],root)==bound['source_sha'],'Candidate SHA changed')
    c.require(command(['git','rev-parse','HEAD^'],root)==P1E_SOURCE,'Candidate does not directly inherit P1e')
    c.require(command(['git','status','--porcelain'],root)=='','Candidate source modified')
    c.require(set(command(['git','diff','--name-only',P1E_SOURCE,bound['source_sha']],root).splitlines())==set(bound['test_only_file_sha256']),'Unexpected test-only delta')
    import subprocess
    parent_manifest=subprocess.check_output(['git','show',P1E_SOURCE+':'+MANIFEST],cwd=root)
    c.require((root/MANIFEST).read_bytes()==parent_manifest+REGISTRATION.encode(),'Candidate manifest differs beyond exact test-only append')
    for name,digest in bound['test_only_file_sha256'].items():
        c.require(c.sha(c.safe_file(root,name))==digest,'New harness source changed')
    for name,digest in bound['inherited_p1e_file_sha256'].items():
        if name==MANIFEST: continue
        c.require(c.sha(c.safe_file(root,name))==digest,'Inherited P1e implementation changed')
    c.require(c.sha(c.safe_file(root,p1.BENCHMARK))==p1.BENCHMARK_SHA,'Inherited benchmark changed')
    features=tomllib.loads((root/'engine/core/Cargo.toml').read_text())['features']
    c.require(features[p1.FEATURE]==[] and p1.FEATURE not in features.get('default',[]),'P1e must remain independent default OFF')
    return {'source_sha':bound['source_sha'],'parent':P1E_SOURCE,'test_only_file_sha256':bound['test_only_file_sha256'],
            'inherited_runtime_unchanged':True,'production_and_KEEP_R1_unchanged':True}

def original_test_files(bound):
    # Manifest is rebuilt by an exact reviewed append, never copied from candidate.
    return sorted([PUBLIC]+[name for name in bound['test_only_file_sha256'] if name!=MANIFEST])

def verify_original(root,bound):
    command=base_ci.command
    c.require(command(['git','rev-parse','HEAD'],root)==ORIGINAL,'Original SHA changed')
    c.require(command(['git','rev-parse','HEAD^'],root)==c.CANONICAL_SOURCE,'Original parent changed')
    import subprocess
    baseline=subprocess.check_output(['git','show','HEAD:'+MANIFEST],cwd=root)
    expected=baseline+(old_ci.TEST_REGISTRATION+REGISTRATION).encode()
    c.require((root/MANIFEST).read_bytes()==expected,'Original manifest not exact test registration')
    c.require(command(['git','diff','--name-only','HEAD'],root).splitlines()==[MANIFEST],'Original runtime changed')
    c.require(sorted(command(['git','ls-files','--others','--exclude-standard'],root).splitlines())==original_test_files(bound),'Original added files changed')
    for name in original_test_files(bound):
        wanted=bound['inherited_p1e_file_sha256'][name] if name==PUBLIC else bound['test_only_file_sha256'][name]
        c.require(c.sha(c.safe_file(root,name))==wanted,'Original harness differs')
    c.require(c.sha(root/p1.BENCHMARK)==p1.BENCHMARK_SHA,'Original benchmark changed')
    return {'source_sha':ORIGINAL,'runtime_unchanged':True,'test_only_additions':original_test_files(bound),
            'manifest_sha256':c.sha(root/MANIFEST)}

def prepare_original(workspace,bound):
    root=workspace/'original'
    c.require(base_ci.command(['git','status','--porcelain'],root)=='','Original must begin clean')
    c.require(base_ci.command(['git','rev-parse','HEAD'],root)==ORIGINAL,'Wrong original checkout')
    path=root/MANIFEST
    path.write_bytes(path.read_bytes()+(old_ci.TEST_REGISTRATION+REGISTRATION).encode())
    for name in original_test_files(bound):
        path=root/name;path.parent.mkdir(parents=True,exist_ok=True)
        with path.open('xb') as out:out.write((workspace/'source'/name).read_bytes())
    return verify_original(root,bound)
