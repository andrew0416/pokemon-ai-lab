"""Stream full-State/Suspension records against the frozen expanded input contract."""
from pathlib import Path
import hashlib, itertools, json, math

FIELDS=set('schema case origin input input_hash decision rolls resume_policy mid_turn activation distribution samples sample_scope flat_factored_full_state_agreement'.split())
META_FIELDS=set('case origin rolls resume_policy mid_turn smoke input_hash decision required_activation'.split())
TOLERANCE=1e-12
SAMPLE_SCOPE='first midturn suspension, Full sampled rolls; compared across binaries only'

def require(ok,message):
    if not ok:raise ValueError(message)

def strict(raw):
    def pairs(items):
        out={}
        for key,value in items:
            require(key not in out,'Duplicate JSON key');out[key]=value
        return out
    return json.loads(raw,object_pairs_hook=pairs,parse_constant=lambda _:(_ for _ in ()).throw(ValueError('Nonfinite JSON')))

def file_sha(path):
    digest=hashlib.sha256()
    with Path(path).open('rb') as stream:
        for chunk in iter(lambda:stream.read(1024*1024),b''):digest.update(chunk)
    return digest.hexdigest()

def validate_contract(contract):
    require(isinstance(contract,dict) and type(contract.get('schema')) is int and contract['schema']==2,'Wrong manifest schema')
    rows=contract.get('cases')
    require(isinstance(rows,list) and rows and len(rows)==contract.get('case_count'),'Incomplete manifest')
    ids=[]
    for row in rows:
        require(isinstance(row,dict) and set(row)==META_FIELDS,'Unexpected manifest row')
        require(isinstance(row['case'],str) and row['case'] and row['case'] not in ids,'Duplicate/empty case ID')
        ids.append(row['case'])
        for key in ('origin','rolls','resume_policy','decision'):
            require(isinstance(row[key],str) and row[key],'Missing manifest '+key)
        require(type(row['input_hash']) is int and 0<=row['input_hash']<2**64 and type(row['smoke']) is bool,'Bad manifest input identity')
        require(isinstance(row['mid_turn'],list) and len(row['mid_turn'])==2
                and all(isinstance(side,list) and all(isinstance(v,str) for v in side) for side in row['mid_turn']),'Invalid resume metadata')
        needed=row['required_activation']
        require(isinstance(needed,list) and needed and len(needed)==len(set(needed))
                and all(isinstance(value,str) and value for value in needed),'Vacuous activation contract')
    require(type(contract.get('distinct_input_count')) is int and 0<contract['distinct_input_count']<=len(rows),'Bad distinct input count')
    require(contract.get('smoke_count')==sum(row['smoke'] for row in rows),'Smoke count differs')
    if 'case_ids' in contract:require(contract['case_ids']==ids,'Manifest order mismatch')
    return rows

def distribution(value,label):
    require(isinstance(value,dict) and value,'Empty '+label)
    for key,prob in value.items():
        require(isinstance(key,str) and key.startswith('State {') and '\n' in key,'Missing full State/Suspension key')
        require(type(prob) in (int,float) and math.isfinite(prob) and 0<=prob<=1,'Invalid '+label+' probability')
    mass=math.fsum(value.values())
    require(abs(mass-1)<=TOLERANCE,'Non-unit '+label+' mass')
    return mass

def validated_rows(path,contract):
    cases=validate_contract(contract)
    with Path(path).open('rb') as stream:
        count=0
        for count,raw in enumerate(stream,1):
            require(raw.endswith(b'\n'),'Incomplete final record')
            require(count<=len(cases),'Extra broad case')
            row=strict(raw);meta=cases[count-1]
            require(isinstance(row,dict) and set(row)==FIELDS,'Unexpected broad record fields')
            require(type(row['schema']) is int and row['schema']==2,'Wrong broad record schema')
            for key in ('case','origin','rolls','resume_policy','mid_turn','input_hash','decision'):
                require(row[key]==meta[key],'Frozen case differs: '+key)
            require(isinstance(row['input'],str) and row['input'].startswith('State {'),'Missing input State')
            require(type(row['input_hash']) is int and 0<=row['input_hash']<2**64,'Invalid input hash')
            activation=row['activation']
            require(isinstance(activation,dict) and all(isinstance(k,str) and type(v) is bool for k,v in activation.items()),'Invalid activation proof')
            require(all(activation.get(key) is True for key in meta['required_activation']),'Required mechanic witness absent/false')
            require(row['flat_factored_full_state_agreement'] is True,'Flat/factored proof missing')
            require(row['sample_scope']==SAMPLE_SCOPE,'Unexpected sampling scope')
            distribution(row['distribution'],'reference')
            distribution(row['samples'],'sample')
            yield row
        require(count==len(cases),'Missing broad cases (smoke is not expanded)')

def compare_records(baseline,candidate,contract):
    rows=validate_contract(contract);results=[];maximum=0.0;total_keys=0;sum_error=0.0;distinct=set();witnesses=set()
    sentinel=object()
    for left,right in itertools.zip_longest(validated_rows(baseline,contract),validated_rows(candidate,contract),fillvalue=sentinel):
        require(left is not sentinel and right is not sentinel,'Record lengths differ')
        for key in FIELDS-{'distribution'}:
            require(left[key]==right[key],left['case']+': changed '+key)
        a,b=left['distribution'],right['distribution']
        require(a.keys()==b.keys(),left['case']+': full State/Suspension support changed')
        errors=[abs(prob-b[key]) for key,prob in a.items()]
        error=max(errors,default=0.0);l1=math.fsum(errors)
        require(error<=TOLERANCE,left['case']+': probability mismatch')
        require(l1/2<=1e-9,left['case']+': total variation exceeds tolerance')
        maximum=max(maximum,error);sum_error+=l1;total_keys+=len(a)
        distinct.add((left['input'],left['decision']))
        witnesses.update(key for key,value in left['activation'].items() if value)
        results.append({'case':left['case'],'full_state_suspension_keys':len(a),'max_abs_probability_error':error,
                        'summed_abs_probability_error':l1,'total_variation':l1/2,'total_variation_tolerance':1e-9,'baseline_mass':math.fsum(a.values()),'candidate_mass':math.fsum(b.values()),
                        'verified_activation':sorted(key for key,value in left['activation'].items() if value)})
    require(len(results)==len(rows),'Incomplete comparison')
    require(len(distinct)==contract['distinct_input_count'],'Distinct input/decision count differs from frozen contract')
    return {'passed':True,'cases':len(results),'distinct_input_count':len(distinct),'full_state_suspension_keys':total_keys,
            'support_exact':True,'input_decision_resume_metadata_exact':True,'sample_distributions_exact':True,
            'activation_proofs_exact':True,'verified_activation_labels':sorted(witnesses),
            'probability_abs_tolerance':TOLERANCE,'max_abs_probability_error':maximum,'sum_case_abs_probability_error':sum_error,
            'baseline_sha256':file_sha(baseline),'candidate_sha256':file_sha(candidate),'case_results':results,
            'scope':'Fixed expanded mechanic/HP/resumption corpus; flat/factored and cross-binary full-State evidence, not a universal mechanics proof or speed measurement.'}
