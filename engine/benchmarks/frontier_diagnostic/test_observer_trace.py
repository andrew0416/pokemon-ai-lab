import copy
import json
import unittest
import observer_trace as t

def event(name='enumeration_begin',ordinal=1,identity=1):
    schema=t.contract();row={key:0 for key in schema['fields']}
    row.update(schema=1,event=name,enumeration=identity,ordinal=ordinal,phase='idle',elapsed_ns=ordinal,
        reason='none',unit=-1,span=0,caller_file='',inclusive_ns={p:0 for p in schema['phases']})
    return row
def stream(rows):return b''.join(b'P17_FRONTIER '+json.dumps(row,separators=(',',':')).encode()+b'\n' for row in rows)

class Trace(unittest.TestCase):
    def test_completed_nested_sessions_and_output(self):
        rows=[event(),event(identity=2),event('enumeration_end',2,2),event('enumeration_end',2),event('output_begin',3),event('output_end',4)]
        result=t.parse(stream(rows),True)
        self.assertEqual(result['enumerations'],2);self.assertEqual(result['event_count'],6)
    def test_live_component_decrease_is_valid_but_cumulative_decrease_is_not(self):
        begin=event();progress=event('progress',2);end=event('enumeration_end',3)
        progress['live_reached']=4;progress['components_created']=4;end['components_created']=4;end['components_committed']=4
        t.parse(stream([begin,progress,end]),True)
        end['components_created']=3
        with self.assertRaises(ValueError):t.parse(stream([begin,progress,end]),True)
    def test_timeout_tail_preserved_but_complete_or_middle_corruption_rejected(self):
        raw=stream([event(),event('progress',2)])+b'P17_FRONTIER {"schema":'
        self.assertGreater(t.parse(raw,False)['truncated_tail_bytes'],0)
        for bad,complete in ((raw,True),(raw+b'\n',False),(stream([event()]),True),(b'',False)):
            with self.subTest(bad=bad[-35:],complete=complete),self.assertRaises(ValueError):t.parse(bad,complete)
    def test_schema_unknown_duplicate_nonfinite_boolean_and_order_fail_closed(self):
        for fault in ('extra','missing','phase','bool','ordinal','duplicate','nan','conservation','request'):
            with self.subTest(fault=fault):
                rows=[event(),event('enumeration_end',2)]
                if fault=='extra':rows[1]['extra']=1
                elif fault=='missing':del rows[1]['group']
                elif fault=='phase':rows[1]['phase']='unknown'
                elif fault=='bool':rows[1]['groups_started']=True
                elif fault=='ordinal':rows[1]['ordinal']=3
                elif fault=='conservation':rows[1]['components_created']=1
                elif fault=='request':rows[1]['event']='first_lazy_request'
                raw=stream(rows)
                if fault=='duplicate':raw=raw.replace(b'"schema":1',b'"schema":1,"schema":1')
                if fault=='nan':raw=raw.replace(b'"elapsed_ns":2',b'"elapsed_ns":NaN')
                with self.assertRaises(ValueError):t.parse(raw,True)
if __name__=='__main__':unittest.main()
