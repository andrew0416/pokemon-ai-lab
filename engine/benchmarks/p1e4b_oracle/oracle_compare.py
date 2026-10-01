"""Strict original concrete-stream export validation, separate from candidate factored schema."""
import itertools
from pathlib import Path
import sys
HERE=Path(__file__).resolve().parent
sys.path.insert(0,str(HERE.parent/'p1e2_adoption'))
import compare_joint as joint
from compare_broad import strict,require,file_sha

LIMITS={'chunk_rows':65536,'merge_fan_in':16,'dictionary_entries_per_live_dictionary':4096,
 'dictionary_debug_bytes_per_live_dictionary':134217728,'scratch_bytes_cumulative':4294967296,
 'max_unique_rows':10000000,'max_replays':1000000000,'max_instruction_rows':10000000000,'max_stages':64}
FIELDS=set('schema kind status method metric_schema dictionary joint selection party_lengths hp_unit_count hp_unit_order record_bytes record_layout record_order probability_policy dictionary_entries suspended_dictionary_entries unique_joint_rows joint_mass tv_bound joint_bytes dictionary_bytes selection_bytes payload_bytes_excluding_manifest max_export_bytes_including_manifest limits scratch_bytes_written total_replays instruction_rows stage_reports caller_state_immutable all_state_restored all_lazy_tags_clear actual_eq_hash_dictionary debug_injective_on_observed_keys suspension_scope oracle_ns timing_scope adoption_approved full500_complete'.split())
TEXT={k:joint.TEXT_CONTRACT[k] for k in ('metric_schema','hp_unit_order','record_layout','record_order','suspension_scope')}
TEXT.update(method='original concrete run_stage + Full Chooser replay; external-memory exact joint-key stage merging',
 probability_policy='raw original chance probabilities; no renormalization, pruning, clamping, candidate filtering or HP independence; compensated duplicate sums',
 timing_scope='diagnostic concrete oracle computation including external sort and disk I/O; not a performance benchmark')
STAGE_FIELDS=set('stage input_rows input_dictionary_entries replays next_rows next_dictionary_entries finished_dictionary_entries active_mass finished_mass'.split())

def read_oracle(directory,plan):
    root=Path(directory)
    require(root.is_dir() and not root.is_symlink(),'Missing oracle export')
    names={'manifest.json','dictionary.json','selection.json','joint.bin'}
    require({p.name for p in root.iterdir()}==names,'Incomplete/extra oracle export files')
    paths={name:joint.safe_file(root,name) for name in names};m=strict(paths['manifest.json'].read_bytes())
    require(isinstance(m,dict) and set(m)==FIELDS,'Unexpected streaming manifest fields')
    require(type(m['schema']) is int and m['schema']==1 and m['kind']=='exact-stream-oracle-export' and m['status']=='complete','Incomplete or mixed oracle method')
    for key,value in TEXT.items():require(m[key]==value,'Streaming contract differs: '+key)
    require({k:m[k] for k in ('dictionary','joint','selection')}=={'dictionary':'dictionary.json','joint':'joint.bin','selection':'selection.json'},'Oracle filenames differ')
    require(m['party_lengths']==[6,6] and type(m['hp_unit_count']) is int and m['hp_unit_count']==12 and type(m['record_bytes']) is int and m['record_bytes']==joint.ROW.size,'Oracle layout differs')
    integers='dictionary_entries suspended_dictionary_entries unique_joint_rows joint_bytes dictionary_bytes selection_bytes payload_bytes_excluding_manifest max_export_bytes_including_manifest scratch_bytes_written total_replays instruction_rows oracle_ns'.split()
    for key in integers:require(joint.integer(m[key]),'Invalid oracle integer '+key)
    require(0<m['dictionary_entries']<=m['unique_joint_rows']<=joint.MAX_ROWS and m['suspended_dictionary_entries']<=m['dictionary_entries'],'Invalid oracle support counts')
    require(m['limits']==LIMITS and all(type(v) is int for v in m['limits'].values()) and m['max_export_bytes_including_manifest']==joint.MAX_BYTES,'Oracle limits differ')
    for field,limit in [('scratch_bytes_written','scratch_bytes_cumulative'),('total_replays','max_replays'),('instruction_rows','max_instruction_rows')]:require(m[field]<=LIMITS[limit],'Oracle guard exceeded: '+field)
    require(joint.number(m['joint_mass']) and abs(m['joint_mass']-1)<=joint.MASS_EPS,'Invalid oracle mass')
    require(type(m['tv_bound']) in (int,float) and m['tv_bound']==0,'Approximate oracle forbidden')
    for key in ('caller_state_immutable','all_state_restored','all_lazy_tags_clear','actual_eq_hash_dictionary','debug_injective_on_observed_keys'):require(m[key] is True,'Missing oracle proof '+key)
    require(m['adoption_approved'] is False and m['full500_complete'] is False,'Unjustified oracle claim')
    stages=m['stage_reports'];require(isinstance(stages,list) and 0<len(stages)<=LIMITS['max_stages'],'Invalid stage reports')
    require(stages[0]['input_rows']==1 and stages[0]['input_dictionary_entries']==1,'Initial stage differs')
    replay_sum=0
    for index,stage in enumerate(stages):
        require(isinstance(stage,dict) and set(stage)==STAGE_FIELDS,'Stage fields differ')
        for key in STAGE_FIELDS-{'active_mass','finished_mass'}:require(joint.integer(stage[key]),'Invalid stage count')
        require(stage['stage']==index and stage['input_rows']>0 and 0<stage['input_dictionary_entries']<=LIMITS['dictionary_entries_per_live_dictionary'],'Invalid stage sequence/input')
        require(stage['input_rows']<=joint.MAX_ROWS and stage['next_rows']<=joint.MAX_ROWS,'Stage row bound exceeded')
        for key in ('next_dictionary_entries','finished_dictionary_entries'):require(stage[key]<=LIMITS['dictionary_entries_per_live_dictionary'],'Stage dictionary bound exceeded')
        for key in ('active_mass','finished_mass'):require(joint.number(stage[key]) and 0<=stage[key]<=1+joint.MASS_EPS,'Invalid stage mass')
        require(abs(stage['active_mass']+stage['finished_mass']-1)<=joint.MASS_EPS,'Stage mass lost')
        require(stage['replays']>=stage['input_rows'],'Missing input replay')
        if index:
            previous=stages[index-1]
            require(stage['input_rows']==previous['next_rows'] and stage['input_dictionary_entries']==previous['next_dictionary_entries'],'Stage chain differs')
            require(stage['finished_dictionary_entries']>=previous['finished_dictionary_entries'] and stage['finished_mass']+joint.MASS_EPS>=previous['finished_mass'],'Finished mass/dictionary regressed')
        replay_sum+=stage['replays']
    require(replay_sum==m['total_replays'],'Replay count differs')
    require(stages[-1]['next_rows']==0 and stages[-1]['active_mass']==0 and stages[-1]['next_dictionary_entries']==0,'Oracle has unfinished stages')
    require(m['dictionary_entries']==stages[-1]['finished_dictionary_entries']<=LIMITS['dictionary_entries_per_live_dictionary'],'Final dictionary differs')
    require(abs(stages[-1]['finished_mass']-m['joint_mass'])<=joint.MASS_EPS,'Final finished/joint mass differs')
    expected=plan.read_bytes() if isinstance(plan,Path) else plan
    require(isinstance(expected,bytes) and paths['selection.json'].read_bytes()==expected,'Oracle selection differs')
    dictionary=strict(paths['dictionary.json'].read_bytes())
    require(isinstance(dictionary,dict) and set(dictionary)=={'schema','kind','party_lengths','entries'} and type(dictionary['schema']) is int and dictionary['schema']==1 and dictionary['kind']=='full-non-hp-state-suspension-dictionary' and dictionary['party_lengths']==[6,6],'Oracle dictionary type differs')
    entries=dictionary['entries']
    require(isinstance(entries,list) and len(entries)==m['dictionary_entries'] and all(isinstance(v,str) and v.startswith('State {') and '\n' in v for v in entries),'Incomplete full-State oracle dictionary')
    require(entries==sorted(set(entries)),'Oracle dictionary not sorted unique')
    sizes={name:path.stat().st_size for name,path in paths.items()}
    require(sizes['joint.bin']==m['joint_bytes']==m['unique_joint_rows']*joint.ROW.size,'Oracle joint bytes differ')
    require(sizes['dictionary.json']==m['dictionary_bytes'] and sizes['selection.json']==m['selection_bytes'],'Oracle metadata bytes differ')
    require(m['payload_bytes_excluding_manifest']==sum(sizes[k] for k in names-{'manifest.json'}) and sum(sizes.values())<=joint.MAX_BYTES,'Oracle payload bytes differ/exceed bound')
    scan={}
    for _ in joint.iter_joint(paths['joint.bin'],m,scan):pass
    hashes={name:file_sha(path) for name,path in paths.items() if name!='joint.bin'};hashes['joint.bin']=scan['sha256']
    return {'manifest':m,'dictionary':entries,'joint_path':paths['joint.bin'],'mass':scan['mass'],'file_sha256':hashes,'file_bytes':sizes,'rows':scan['rows']}

def compare_exports(oracle,candidate,plan):
    left=read_oracle(oracle,plan);right=joint.read_export(candidate,plan)
    require(left['dictionary']==right['dictionary'],'Full non-HP State/Suspension dictionary changed')
    a,b={},{};maximum=0.;l1=joint.Sum();tv=joint.Sum();rows=0;sentinel=object()
    for first,second in itertools.zip_longest(joint.iter_joint(left['joint_path'],left['manifest'],a),joint.iter_joint(right['joint_path'],right['manifest'],b),fillvalue=sentinel):
        require(first is not sentinel and second is not sentinel,'Joint support lengths differ')
        require(first[0]==second[0],'Full12HP joint support differs')
        delta=abs(first[1]-second[1]);require(delta<=joint.POINT_EPS,'Per-key probability differs')
        maximum=max(maximum,delta);l1.add(delta);tv.add(abs(first[1]/left['mass']-second[1]/right['mass'])/2);rows+=1
    require(a['sha256']==left['file_sha256']['joint.bin'] and b['sha256']==right['file_sha256']['joint.bin'],'Joint file changed during comparison')
    require(tv.total()<=joint.MASS_EPS,'Normalized TV exceeds tolerance')
    return {'passed':True,'dictionary_exact':True,'full_joint_12hp_support_exact':True,'dictionary_entries':len(left['dictionary']),
      'unique_joint_rows':rows,'max_abs_probability_error':maximum,'summed_abs_raw_probability_error':l1.total(),
      'normalized_tv':tv.total(),'oracle_raw_mass':left['mass'],'candidate_raw_mass':right['mass'],
      'point_tolerance':joint.POINT_EPS,'mass_and_tv_tolerance':joint.MASS_EPS,
      'oracle_file_sha256':left['file_sha256'],'candidate_file_sha256':right['file_sha256'],
      'oracle_method':left['manifest']['method'],'candidate_method':right['manifest']['method'],
      'adoption_approved':False,'full500_complete':False,'speed_ratio':None}
