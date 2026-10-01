"""Validate and stream-compare complete full non-HP State/Suspension + joint HP exports."""
from pathlib import Path
import hashlib,itertools,math,struct
from compare_broad import strict,require,file_sha

ROW=struct.Struct('<I12hd')
MASS_EPS=1e-9
POINT_EPS=1e-12
MAX_ROWS=10000000
MAX_BYTES=536870912
MANIFEST_FIELDS=set('schema kind status method metric_schema dictionary joint selection party_lengths hp_unit_count hp_unit_order record_bytes record_layout record_order probability_policy dictionary_entries components suspended_components unique_joint_rows flat_count_upper_bound component_mass joint_mass tv_bound joint_bytes dictionary_bytes selection_bytes payload_bytes_excluding_manifest limits all_state_restored all_lazy_tags_clear actual_eq_hash_dictionary debug_injective_on_observed_keys suspension_scope kernel_ns prepare_ns export_ns_before_manifest timing_scope'.split())
TEXT_CONTRACT={
 'method':'unmodified public enumerate_turn_factored_with Full max_support=None',
 'metric_schema':'full State plus full Suspension; only all current Pokemon.hp values projected and restored by joint tuple',
 'hp_unit_order':'SideId::One party index ascending, then SideId::Two party index ascending; all members including inactive, fainted and empty',
 'record_layout':'little-endian u32 dictionary_id; hp_unit_count signed i16 HP; IEEE754 binary64 probability; no header or padding',
 'record_order':'dictionary_id ascending then signed HP tuple lexicographic; duplicate tuples summed',
 'probability_policy':'raw probabilities, no renormalization, pruning, clamping, or zero dropping; positive finite; factor/component/joint mass within 1e-9 of one; Neumaier sums',
 'suspension_scope':'first mid-turn pause, no automatic resume; complete pending state included in key',
 'timing_scope':'kernel_ns covers only the one enumeration API call; exporter is accuracy validation, not a paired performance measurement',
}
class Sum:
    def __init__(self):self.value=0.;self.correction=0.
    def add(self,value):
        total=self.value+value
        self.correction+=(self.value-total)+value if abs(self.value)>=abs(value) else (value-total)+self.value
        self.value=total
    def total(self):return self.value+self.correction

def number(value):return type(value) in (int,float) and math.isfinite(value)
def integer(value):return type(value) is int and value>=0
def safe_file(root,name):
    require(name in ('manifest.json','dictionary.json','selection.json','joint.bin'),'Unapproved export path')
    path=Path(root)/name
    require(path.is_file() and not path.is_symlink() and path.resolve().is_relative_to(Path(root).resolve()),'Missing/unsafe export file')
    return path

def iter_joint(path,manifest,proof):
    previous=None;count=0;seen=set();mass=Sum();digest=hashlib.sha256()
    with Path(path).open('rb') as stream:
        while True:
            block=stream.read(ROW.size*16384)
            if not block:break
            require(len(block)%ROW.size==0,'Truncated joint row')
            digest.update(block)
            for row in ROW.iter_unpack(block):
                key=row[:-1];prob=row[-1];identifier=key[0];hp=key[1:]
                require(identifier<manifest['dictionary_entries'],'Dictionary id out of range')
                require(all(value>=0 for value in hp),'Negative HP tuple')
                require(previous is None or previous<key,'Unsorted/duplicate joint support')
                require(math.isfinite(prob) and 0<prob<=1+MASS_EPS,'Invalid raw joint probability')
                previous=key;count+=1;seen.add(identifier);mass.add(prob)
                require(count<=manifest['unique_joint_rows'] and count<=MAX_ROWS,'Extra joint rows')
                yield key,prob
    require(count==manifest['unique_joint_rows'],'Missing joint rows')
    require(seen==set(range(manifest['dictionary_entries'])),'Unused/empty dictionary entries')
    rawmass=mass.total()
    require(abs(rawmass-1)<=MASS_EPS,'Raw joint mass is not one')
    require(abs(rawmass-manifest['joint_mass'])<=POINT_EPS,'Manifest joint mass differs from rows')
    proof.update(rows=count,mass=rawmass,sha256=digest.hexdigest())

def read_export(directory,expected_plan,limits=None):
    root=Path(directory);limits=limits or {'max_rows':MAX_ROWS,'max_bytes_including_manifest':MAX_BYTES}
    require(limits=={'max_rows':MAX_ROWS,'max_bytes_including_manifest':MAX_BYTES},'Unreviewed export limits')
    require(root.is_dir() and not root.is_symlink(),'Missing export directory')
    require({p.name for p in root.iterdir()}=={'manifest.json','dictionary.json','selection.json','joint.bin'},'Incomplete/extra export files')
    paths={name:safe_file(root,name) for name in ('manifest.json','dictionary.json','selection.json','joint.bin')}
    manifest=strict(paths['manifest.json'].read_bytes())
    require(isinstance(manifest,dict) and set(manifest)==MANIFEST_FIELDS,'Unexpected manifest fields')
    require(type(manifest['schema']) is int and manifest['schema']==1 and manifest['kind']=='exact-joint-hp-export'
            and manifest['status']=='complete','Export is incomplete')
    for key,value in TEXT_CONTRACT.items():require(manifest[key]==value,'Exporter contract changed: '+key)
    require({k:manifest[k] for k in ('dictionary','joint','selection')}=={'dictionary':'dictionary.json','joint':'joint.bin','selection':'selection.json'},'Export filenames differ')
    require(manifest['party_lengths']==[6,6] and type(manifest['hp_unit_count']) is int and manifest['hp_unit_count']==12
            and type(manifest['record_bytes']) is int and manifest['record_bytes']==ROW.size,'Joint row layout differs')
    for key in ('dictionary_entries','components','suspended_components','unique_joint_rows','flat_count_upper_bound',
                'joint_bytes','dictionary_bytes','selection_bytes','payload_bytes_excluding_manifest','kernel_ns','prepare_ns','export_ns_before_manifest'):
        require(integer(manifest[key]),'Invalid manifest integer '+key)
    require(0<manifest['dictionary_entries']<=manifest['components']<=manifest['flat_count_upper_bound'],'Invalid component/dictionary counts')
    require(manifest['dictionary_entries']<=manifest['unique_joint_rows']<=manifest['flat_count_upper_bound']
            and manifest['unique_joint_rows']<=MAX_ROWS and manifest['suspended_components']<=manifest['components'],'Invalid support count')
    require(manifest['limits']==limits,'Export resource limits differ')
    require(type(manifest['tv_bound']) in (int,float) and manifest['tv_bound']==0,'Approximate reference forbidden')
    for key in ('component_mass','joint_mass'):
        require(number(manifest[key]) and abs(manifest[key]-1)<=MASS_EPS,'Invalid raw mass '+key)
    require(abs(manifest['component_mass']-manifest['joint_mass'])<=MASS_EPS,'Component/joint mass differs')
    for key in ('all_state_restored','all_lazy_tags_clear','actual_eq_hash_dictionary','debug_injective_on_observed_keys'):
        require(manifest[key] is True,'Missing exporter correctness proof '+key)
    plan=expected_plan.read_bytes() if isinstance(expected_plan,Path) else expected_plan
    require(isinstance(plan,bytes) and paths['selection.json'].read_bytes()==plan,'Exported selection differs from original frozen plan bytes')
    dictionary=strict(paths['dictionary.json'].read_bytes())
    require(isinstance(dictionary,dict) and set(dictionary)=={'schema','kind','party_lengths','entries'},'Dictionary fields differ')
    require(type(dictionary['schema']) is int and dictionary['schema']==1 and dictionary['kind']=='full-non-hp-state-suspension-dictionary'
            and dictionary['party_lengths']==[6,6],'Dictionary type differs')
    entries=dictionary['entries']
    require(isinstance(entries,list) and len(entries)==manifest['dictionary_entries']
            and all(isinstance(v,str) and v.startswith('State {') and '\n' in v for v in entries),'Incomplete full-State dictionary')
    require(entries==sorted(set(entries)),'Dictionary is not unique and sorted')
    sizes={name:path.stat().st_size for name,path in paths.items()}
    require(sizes['joint.bin']==manifest['joint_bytes']==manifest['unique_joint_rows']*ROW.size,'Joint byte count differs')
    require(sizes['dictionary.json']==manifest['dictionary_bytes'] and sizes['selection.json']==manifest['selection_bytes'],'Metadata size differs')
    require(manifest['payload_bytes_excluding_manifest']==sizes['joint.bin']+sizes['dictionary.json']+sizes['selection.json'],'Payload byte accounting differs')
    require(sum(sizes.values())<=MAX_BYTES,'Export exceeds bound including manifest')
    scan={}
    for _ in iter_joint(paths['joint.bin'],manifest,scan):pass
    hashes={name:file_sha(path) for name,path in paths.items() if name!='joint.bin'}
    hashes['joint.bin']=scan['sha256']
    return {'manifest':manifest,'dictionary':entries,'joint_path':paths['joint.bin'],'mass':scan['mass'],
            'file_sha256':hashes,'file_bytes':sizes,'rows':scan['rows']}

def compare_exports(baseline,candidate,expected_plan):
    left=read_export(baseline,expected_plan);right=read_export(candidate,expected_plan)
    require(left['dictionary']==right['dictionary'],'Full non-HP State/Suspension dictionary changed')
    a,b={},{};maximum=0.;raw_l1=Sum();tv=Sum();rows=0;sentinel=object()
    for first,second in itertools.zip_longest(iter_joint(left['joint_path'],left['manifest'],a),
                                              iter_joint(right['joint_path'],right['manifest'],b),fillvalue=sentinel):
        require(first is not sentinel and second is not sentinel,'Joint support lengths differ')
        require(first[0]==second[0],'Joint full-party HP support changed')
        diff=abs(first[1]-second[1]);require(diff<=POINT_EPS,'Per-key raw probability differs')
        maximum=max(maximum,diff);raw_l1.add(diff);tv.add(abs(first[1]/left['mass']-second[1]/right['mass'])/2);rows+=1
    require(a['sha256']==left['file_sha256']['joint.bin'] and b['sha256']==right['file_sha256']['joint.bin'],'Joint file changed during comparison')
    require(tv.total()<=MASS_EPS,'Normalized total variation exceeds tolerance')
    return {'passed':True,'full_non_hp_state_suspension_dictionary_exact':True,'full_joint_party_hp_support_exact':True,
            'dictionary_entries':len(left['dictionary']),'unique_joint_rows':rows,
            'per_key_raw_probability_abs_tolerance':POINT_EPS,'max_abs_probability_error':maximum,
            'summed_abs_raw_probability_error':raw_l1.total(),'baseline_raw_mass':left['mass'],'candidate_raw_mass':right['mass'],
            'raw_mass_tolerance':MASS_EPS,'normalized_tv':tv.total(),'normalized_tv_tolerance':MASS_EPS,
            'baseline_components':left['manifest']['components'],'candidate_components':right['manifest']['components'],
            'baseline_file_sha256':left['file_sha256'],'candidate_file_sha256':right['file_sha256'],
            'scope':'Complete joint full-party HP distribution with exact full non-HP State and Suspension keys for the frozen single case; no marginal-only equivalence or speed claim.',
            'official_full500_metrics':None,'full500_complete':False}
