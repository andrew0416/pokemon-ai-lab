import unittest
from protocol import quality, select, score_trial

def snap(time=1,calls=10,p=(.5,.5),q=(.5,.5)):
    return dict(kind='policy',elapsed_s=time,transitions=calls,rows=list(p),cols=list(q),
        estimated_value=0.,local_gap=0.)

class ProtocolTests(unittest.TestCase):
    def test_late_result_not_backdated(self):
        rows=[snap(.1,10),snap(.3,20)]
        self.assertIsNone(select(rows,.05))
        self.assertIs(select(rows,.2),rows[0])
        self.assertIs(select(rows,15,'transitions'),rows[0])
    def test_reference_gap_different_from_zero_local_gap(self):
        ref=dict(rows=2,cols=2,matrix=[1.,-1.,-1.,1.],cell_lower=[1.,-1.,-1.,1.],
            cell_upper=[1.,-1.,-1.,1.],row_policy=[.5,.5],col_policy=[.5,.5],value=0.,lower=0.,upper=0.)
        self.assertEqual(quality(snap(),ref)['reference_root_br_gap'],0.)
        bad=quality(snap(p=(1.,0.),q=(1.,0.)),ref)
        self.assertEqual(bad['reference_root_br_gap'],2.)
        self.assertEqual(bad['reference_gap_upper'],2.)
    def test_domain_error_invalidates_incumbents(self):
        raw=[dict(kind='ready',actions=[['a','b'],['x','y']]),snap(),dict(kind='error',error='Unsupported')]
        score=score_trial(raw,None,[2],[20])
        self.assertFalse(score['valid'])
        self.assertFalse(score['time'][0]['available'])
    def test_bad_probability_is_not_normalized_away(self):
        raw=[dict(kind='ready',actions=[['a','b'],['x','y']]),snap(p=(.1,.1))]
        with self.assertRaises(AssertionError): score_trial(raw,None,[2],[20])
    def test_value_interval_propagation(self):
        ref=dict(rows=1,cols=1,matrix=[3.],cell_lower=[2.],cell_upper=[4.],
            row_policy=[1.],col_policy=[1.],value=3.,lower=2.,upper=4.)
        out=quality(snap(p=(1.,),q=(1.,)),ref)
        self.assertEqual(out['reference_gap_lower'],0.)
        self.assertEqual(out['reference_gap_upper'],2.)

if __name__=='__main__': unittest.main()
