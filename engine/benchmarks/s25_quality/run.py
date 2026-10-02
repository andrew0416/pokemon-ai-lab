"""S25b isolated Linux build, sequential same-VM trials and auditable scoring."""
from pathlib import Path
import argparse
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import time

from protocol import PLAN, HERE, digest, write, records, score_trial

ROOT=HERE.parents[2]
FEATURES=','.join(PLAN['features'])
PACKAGE=ROOT/'s25-package'

def command(args, log, cwd=None):
    with Path(log).open('xb') as f:
        p=subprocess.run(args,cwd=cwd or ROOT,stdout=f,stderr=subprocess.STDOUT)
    if p.returncode:
        print(Path(log).read_text(errors='replace')[-6000:],flush=True)
        raise RuntimeError(f'command failed ({p.returncode}): {args[0]}')

def git(*args):
    return subprocess.check_output(['git','-C',str(ROOT),*args],text=True).strip()

def source():
    actual=git('rev-parse','HEAD')
    assert actual==os.environ['GITHUB_SHA']
    assert git('rev-parse','HEAD^')==PLAN['parent_source']
    assert not git('status','--porcelain','--untracked-files=no'), 'modified tracked source'
    return actual

def inputs(case):
    scenario=ROOT/case['scenario']
    result={case['scenario']:digest(scenario)}
    data=json.loads(scenario.read_bytes())
    for side in ('p1','p2'):
        value=data[side]['team']
        if isinstance(value,str):
            team=(scenario.parent/value).resolve()
            assert team.is_relative_to(ROOT.resolve())
            result[team.relative_to(ROOT).as_posix()]=digest(team)
    return result

def build():
    sha=source()
    PACKAGE.mkdir(exist_ok=False)
    command([sys.executable,'-B','-m','unittest','discover','-s',str(HERE),'-p','test_*.py','-v'],PACKAGE/'protocol-tests.log')
    # The same immutable binary will run every algorithm, with all runtime observers OFF.
    cargo=['cargo','--locked']
    command(['cargo','test','--locked','--release','-p','lab-search','--lib','--features',FEATURES],PACKAGE/'on-tests.log',ROOT/'engine')
    command(['cargo','test','--locked','--release','-p','lab-search','--bin','lab-search-quality','--features',FEATURES],PACKAGE/'reference-test.log',ROOT/'engine')
    command(['cargo','test','--locked','-p','lab-search','--lib'],PACKAGE/'off-tests.log',ROOT/'engine')
    counts=[]
    for name,minimum in [('on-tests.log',36),('reference-test.log',1),('off-tests.log',17)]:
        text=(PACKAGE/name).read_text()
        matches=re.findall(r'test result: ok\. (\d+) passed; 0 failed;',text)
        assert matches and int(matches[-1])>=minimum, name
        counts.append(dict(log=name,passed=int(matches[-1])))
    command(['cargo','build','--locked','--release','-p','lab-search','--bin','lab-search-quality','--features',FEATURES],PACKAGE/'build.log',ROOT/'engine')
    binary=ROOT/'target-s25/release/lab-search-quality'
    assert binary.is_file()
    shutil.copy2(binary,PACKAGE/'lab-search-quality')
    fingerprint=json.loads(subprocess.check_output([str(binary),'--fingerprint']))
    assert all(fingerprint[k] is True for k in ('quality_feature','budgeted_feature','prepared','leaf_endings','borrowed_keys'))
    meta=json.loads(subprocess.check_output(['cargo','metadata','--locked','--format-version','1','--features',FEATURES],cwd=ROOT/'engine'))
    names={p['id']:p['name'] for p in meta['packages']}
    resolved={names[n['id']]:n['features'] for n in meta['resolve']['nodes'] if names[n['id']] in ('lab-engine','lab-search')}
    allowed={f.split('/',1)[1] for f in PLAN['features']}|{'experiment-budgeted-search','experiment-prepared-turn','experiment-leaf-ending-states'}
    for features in resolved.values():
        assert all(not f.startswith('experiment-') or f in allowed for f in features), features
        assert not any('observer' in f or 'observe' in f for f in features)
    assert {'experiment-hurt-readers','experiment-compact-volatiles','experiment-replay-action-keys'}<=set(resolved['lab-engine'])
    record=dict(schema=1,source_sha=sha,parent=PLAN['parent_source'],plan_sha256=digest(HERE/'plan.json'),
        binary_sha256=digest(PACKAGE/'lab-search-quality'),fingerprint=fingerprint,resolved_features=resolved,
        tests=counts,rustc=subprocess.check_output(['rustc','-vV'],text=True),
        cargo_env={k:v for k,v in os.environ.items() if k.startswith(('CARGO_PROFILE_','RUSTFLAGS','CARGO_BUILD_'))},
        cases={c['id']:inputs(c) for c in PLAN['cases']})
    write(PACKAGE/'manifest.json',record)
    with open(os.environ['GITHUB_OUTPUT'],'a',encoding='utf-8') as f:
        f.write('manifest_sha256='+digest(PACKAGE/'manifest.json')+'\n')
    print(json.dumps(dict(build='passed',tests=counts,manifest_sha256=digest(PACKAGE/'manifest.json'))),flush=True)

def verify_package():
    sha=source()
    assert digest(PACKAGE/'manifest.json')==os.environ['PACKAGE_MANIFEST_SHA']
    manifest=json.loads((PACKAGE/'manifest.json').read_bytes())
    assert manifest['source_sha']==sha and manifest['plan_sha256']==digest(HERE/'plan.json')
    assert manifest['binary_sha256']==digest(PACKAGE/'lab-search-quality')
    for case in PLAN['cases']:
        assert manifest['cases'][case['id']]==inputs(case)
    (PACKAGE/'lab-search-quality').chmod(0o755)
    assert json.loads(subprocess.check_output([str(PACKAGE/'lab-search-quality'),'--fingerprint']))==manifest['fingerprint']
    return manifest

def trial(folder, case, mode, seed, limit):
    """Only the owned executable is killed. OS wait4 returns exact process CPU / peak RSS.

    Internal timestamps define eligibility; the external watchdog is a safety cap, not
    a license to use a post-deadline policy. RSS/CPU cover the whole process, including setup.
    """
    import resource
    folder.mkdir(exist_ok=False)
    cpu=min(os.sched_getaffinity(0))
    def restrict():
        os.sched_setaffinity(0,{cpu})
        size=PLAN['address_space_limit_gib']*1024**3
        resource.setrlimit(resource.RLIMIT_AS,(size,size))
    argv=[str(PACKAGE/'lab-search-quality'),str(ROOT/case['scenario']),mode,case['rolls'],
          str(case['position']),str(seed),str(round(limit*1000))]
    raw=folder/'stdout.jsonl'
    launched=time.monotonic(); ready_at=None; reason=None; usage=None
    with raw.open('xb') as out, (folder/'stderr.log').open('xb') as err:
        proc=subprocess.Popen(argv,cwd=ROOT,stdout=out,stderr=err,preexec_fn=restrict,start_new_session=True)
        while True:
            pid,status,ru=os.wait4(proc.pid,os.WNOHANG)
            if pid:
                proc.returncode=os.waitstatus_to_exitcode(status);usage=ru;break
            now=time.monotonic()
            if ready_at is None:
                # Before the first policy, the only complete line is the tiny ready record.
                with raw.open('rb') as f: line=f.readline()
                if line.endswith(b'\n'):
                    r=json.loads(line)
                    if r.get('kind')=='ready':ready_at=now
            if reason is None:
                if ready_at is not None and now-ready_at>limit+0.25:
                    reason='deadline';proc.kill()
                elif ready_at is None and now-launched>60:
                    reason='setup-timeout';proc.kill()
            time.sleep(.01)
    raw_records=records(raw)
    result=dict(mode=mode,seed=seed,limit_s=limit,exit_code=proc.returncode,watchdog=reason,
        process_wall_s=time.monotonic()-launched,cpu_user_s=usage.ru_utime,cpu_system_s=usage.ru_stime,
        process_peak_rss_kib=usage.ru_maxrss,cpu_affinity=[cpu],stdout_sha256=digest(raw),
        stderr_sha256=digest(folder/'stderr.log'),argv=argv)
    write(folder/'process.json',result)
    return result,raw_records

def case_run(case_id):
    manifest=verify_package()
    case=next(c for c in PLAN['cases'] if c['id']==case_id)
    out=ROOT/('s25-case-'+case_id);out.mkdir(exist_ok=False)
    write(out/'environment.json',dict(platform=platform.platform(),processor=platform.processor(),
        cpuinfo=Path('/proc/cpuinfo').read_text(),source_sha=manifest['source_sha'],
        package_manifest_sha256=digest(PACKAGE/'manifest.json'),case=case))
    reference=None; reference_actions=None; ref_record=dict(status='not-planned-large-case')
    if case['reference']:
        process,raw=trial(out/'reference',case,'reference',1,PLAN['reference_limit_seconds'])
        completed=[r for r in raw if r.get('kind')=='reference']
        if process['exit_code']==0 and len(completed)==1:
            reference=completed[0]['reference']
            reference_actions=next(r['actions'] for r in raw if r.get('kind')=='ready')
            ref_record=dict(status='completed',process=process,report=completed[0])
        else:
            ref_record=dict(status='incomplete',process=process,errors=[r for r in raw if r.get('kind')=='error'])
    # Retain warmups separately; never mix their results into the three measured pairs.
    for mode in ('baseline','selective'):
        trial(out/('warmup-'+mode),case,mode,1,min(1.,max(case['seconds'])))
    trials=[]; menu=reference_actions
    for repeat,seed in enumerate(PLAN['seeds']):
        order=['baseline','selective'] if repeat%2==0 else ['selective','baseline']
        for mode in order:
            process,raw=trial(out/f'pair-{repeat}-{mode}',case,mode,seed,max(case['seconds']))
            ready=next((r for r in raw if r.get('kind')=='ready'),None)
            if ready is None:
                trials.append(dict(repeat=repeat,mode=mode,process=process,valid=False,error='missing-ready'))
                continue
            if menu is None:menu=ready['actions']
            assert ready['actions']==menu, 'action/initial-position drift'
            scored=score_trial(raw,reference,case['seconds'],PLAN['work_checkpoints'])
            # Only intentional deadline interruption preserves earlier completed policies.
            if process['exit_code']!=0 and process['watchdog']!='deadline':
                scored['valid']=False
                for key in ('time','work'):
                    for row in scored[key]:row.update(available=False,quality=None,policy=None)
            trials.append(dict(repeat=repeat,mode=mode,process=process,**scored))
    result=dict(case=case,source_sha=manifest['source_sha'],package_manifest_sha256=digest(PACKAGE/'manifest.json'),
        reference=ref_record,trials=trials,valid=all(t['valid'] for t in trials))
    write(out/'result.json',result)
    print(json.dumps(dict(case=case_id,reference=ref_record['status'],trials=len(trials),valid=result['valid'])),flush=True)

def summary():
    import statistics
    out=ROOT/'s25-summary';out.mkdir(exist_ok=False)
    all_results=[]; rows=[]
    for case in PLAN['cases']:
        paths=list((ROOT/'s25-results').rglob('result.json'))
        matches=[p for p in paths if json.loads(p.read_bytes())['case']['id']==case['id']]
        assert len(matches)==1, f'missing/duplicate case {case["id"]}'
        data=json.loads(matches[0].read_bytes());all_results.append(data)
        assert data['source_sha']==os.environ['GITHUB_SHA']
        assert data['package_manifest_sha256']==os.environ['PACKAGE_MANIFEST_SHA']
        for mode in ('baseline','selective'):
            trials=[t for t in data['trials'] if t['mode']==mode]
            assert len(trials)==len(PLAN['seeds'])
            for i,limit in enumerate(case['seconds']):
                selected=[t['time'][i] for t in trials if t.get('valid')]
                gaps=[r['quality']['reference_root_br_gap'] for r in selected if r['available'] and r['quality'] is not None]
                rows.append(dict(case=case['id'],mode=mode,seconds=limit,
                    available=sum(r['available'] for r in selected),trials=len(trials),
                    reference_status=data['reference']['status'],median_reference_gap=statistics.median(gaps) if gaps else None,
                    median_process_peak_rss_kib=statistics.median(t['process']['process_peak_rss_kib'] for t in trials)))
    result=dict(schema=1,source_sha=os.environ['GITHUB_SHA'],package_manifest_sha256=os.environ['PACKAGE_MANIFEST_SHA'],
        all_trials_valid=all(d['valid'] for d in all_results),
        references_completed=sum(d['reference']['status']=='completed' for d in all_results),
        references_planned=sum(c['reference'] for c in PLAN['cases']),rows=rows,
        limitations=['Perfect information; no hidden-state strength claim.',
        'Reference uses independent traversal but shared engine adapter and RM+; propagated numerical intervals are recorded.',
        'Full action under Sensible pruning and specified roll mode, not an unrestricted full-game certificate.',
        'Old search can evaluate unresolved switches at horizon and use maximin inside switches; this remains an algorithm difference.',
        'Work-budget curves contain completed incumbents before cost thresholds; old solver is externally time-capped, not globally transition-capped.',
        'CPU and peak RSS are whole-process metrics including setup and a possible final overrun; time-cutoff policy eligibility uses internal timestamps.',
        'Three AB/BA pairs form a pilot, not a broad speedup or playing-strength result.'])
    write(out/'summary.json',result)
    lines=['# S25b search-quality pilot','',f"References: {result['references_completed']}/{result['references_planned']}; valid trials: {result['all_trials_valid']}",'',
        '| case | seconds | baseline available | selective available | baseline BR gap | selective BR gap |',
        '|---|---:|---:|---:|---:|---:|']
    for case in PLAN['cases']:
        for limit in case['seconds']:
            b=next(r for r in rows if r['case']==case['id'] and r['seconds']==limit and r['mode']=='baseline')
            s=next(r for r in rows if r['case']==case['id'] and r['seconds']==limit and r['mode']=='selective')
            fmt=lambda v:'—' if v is None else f'{v:.6f}'
            lines.append(f"| {case['id']} | {limit} | {b['available']}/3 | {s['available']}/3 | {fmt(b['median_reference_gap'])} | {fmt(s['median_reference_gap'])} |")
    lines+=['','Lower reference BR gap is better; no policy is missing, not a zero gap.','']+['- '+v for v in result['limitations']]
    (out/'REPORT.md').write_text('\n'.join(lines)+'\n',encoding='utf-8')
    with open(os.environ['GITHUB_STEP_SUMMARY'],'a',encoding='utf-8') as f:f.write('\n'.join(lines)+'\n')
    print(json.dumps({k:v for k,v in result.items() if k not in ('rows','limitations')}))
    if not result['all_trials_valid']:raise RuntimeError('one or more trials failed; raw evidence retained')

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('mode',choices=['build','case','summary']);p.add_argument('--case')
    a=p.parse_args()
    if a.mode=='case':case_run(a.case)
    else:globals()[a.mode]()
