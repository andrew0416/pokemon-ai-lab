"""Strict primitive diagnostic trace parsing; inclusive timers are never summed."""
from collections import Counter
import hashlib
from common import c,HERE

def contract():
    return c.strict_json((HERE/'observer-contract.json').read_bytes())

def parse(raw,complete,required=True):
    schema=contract();prefix=schema['prefix'].encode();rows=[];other=[];tail=b''
    contexts={};counts=Counter()
    pieces=raw.splitlines(keepends=True)
    for number,line in enumerate(pieces,1):
        # SIGKILL can interrupt the multiple writes used to emit the last event.
        if not line.endswith(b'\n'):
            c.require(not complete and number==len(pieces),'Successful trace must end with LF')
            tail=line;continue
        if not line.startswith(prefix):
            other.append(line);continue
        row=c.strict_json(line[len(prefix):]);c.require(isinstance(row,dict) and set(row)==set(schema['fields']),'Observer field set changed')
        c.require(row['schema']==1 and type(row['schema']) is int,'Unknown observer schema')
        c.require(row['event'] in schema['events'] and row['phase'] in schema['phases'] and row['reason'] in schema['reason_values'],'Unknown observer identifier')
        strings={'event','phase','reason','caller_file'};special=strings|{'inclusive_ns','unit','span'}
        for key,value in row.items():
            if key in strings:c.require(isinstance(value,str),'Observer string type changed')
            elif key in ('unit','span'):c.require(type(value) is int and -(2**31)<=value<2**31,'Observer signed type changed')
            elif key not in special:c.require(type(value) is int and 0<=value<2**64,'Observer unsigned type changed')
        durations=row['inclusive_ns'];c.require(isinstance(durations,dict) and set(durations)==set(schema['phases']),'Observer phase timers changed')
        c.require(all(type(v) is int and 0<=v<2**64 for v in durations.values()),'Invalid inclusive timer')
        c.require(row['enumeration']>0 and row['ordinal']>0,'Observer sequence IDs invalid')
        c.require(row['components_created']==row['components_committed']+row['components_discarded']+row['live_reached'],'Component accounting is not conserved')
        if row['event']=='first_lazy_request':
            c.require(row['reason']!='none' and 0<=row['unit']<12 and row['span']>0 and row['caller_file'] and row['caller_line']>0 and row['caller_column']>0,'Invalid first lazy request attribution')
        else:
            c.require(row['caller_file']=='' and row['caller_line']==row['caller_column']==0 and row['reason']=='none' and row['unit']==-1 and row['span']==0,'Unexpected request/caller attribution')
        identity=row['enumeration'];prior=contexts.get(identity)
        if prior is None:
            c.require(row['event']=='enumeration_begin' and row['ordinal']==1,'Missing enumeration begin')
            state={'ended':False,'output_started':False,'output_ended':False,'abort':False}
        else:
            last,state=prior
            c.require(row['ordinal']==last['ordinal']+1 and row['elapsed_ns']>=last['elapsed_ns'] and row['stage']>=last['stage'],'Observer event order changed')
            c.require(row['event']!='enumeration_begin' and not state['abort'] and not state['output_ended'],'Observer event after closed session')
            for key in schema['cumulative_counts']:c.require(row[key]>=last[key],'Cumulative count decreased: '+key)
            for key in durations:c.require(durations[key]>=last['inclusive_ns'][key],'Inclusive timer decreased: '+key)
        event=row['event']
        if event=='enumeration_end':c.require(not state['ended'],'Duplicate enumeration end');state['ended']=True
        elif event=='output_begin':c.require(state['ended'] and not state['output_started'],'Output association invalid');state['output_started']=True
        elif event=='output_end':c.require(state['output_started'],'Output end without begin');state['output_ended']=True
        elif event=='abort':state['abort']=True
        contexts[identity]=(row,state);rows.append(row);counts[event]+=1
    c.require(not required or rows,'Observer did not activate')
    if complete:
        c.require(not other and not tail,'Unexpected successful stderr')
        c.require(all(state['ended'] and not state['abort'] and (not state['output_started'] or state['output_ended']) for _,state in contexts.values()),'Incomplete successful enumeration/output')
    return {'schema':1,'event_count':len(rows),'event_counts':dict(sorted(counts.items())),
        'enumerations':len(contexts),'complete_process':complete,'last_event':rows[-1] if rows else None,
        'last_events_by_enumeration':{str(k):v[0] for k,v in contexts.items()},
        'truncated_tail_bytes':len(tail),'truncated_tail_sha256':hashlib.sha256(tail).hexdigest(),
        'other_stderr_bytes':sum(map(len,other)),'raw_sha256':hashlib.sha256(raw).hexdigest(),
        'timer_semantics':schema['timer_semantics'],'group_semantics':schema['group_semantics'],
        'timing_comparable':False}
