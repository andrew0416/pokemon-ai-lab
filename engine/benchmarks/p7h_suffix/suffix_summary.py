"""Descriptive paired ratios only; no censored or incomplete-case headline."""
import math,statistics
from suffix_contract import c
def schedule():
    rows=[{'phase':'warmup','block':None,'slot':None,'arm':arm} for arm in ('off','on')]
    return rows+[{'phase':'timed','block':block,'slot':slot,'arm':arm} for block in range(3) for slot,arm in enumerate(('off','on','on','off'))]
def case_summary(case_id,values):
    c.require(len(values)==14,'Missing warmup/ABBA rows')
    for value in values:c.require(type(value) is int and value>0,'Invalid API timing')
    blocks=[]
    for block in range(3):
        group=values[2+4*block:6+4*block];off=[group[0],group[3]];on=[group[1],group[2]]
        blocks.append({'block':block,'off_kernel_ns':off,'on_kernel_ns':on,'on_over_off':sum(on)/sum(off)})
    ratio=statistics.median(b['on_over_off'] for b in blocks)
    off=[v for b in blocks for v in b['off_kernel_ns']];on=[v for b in blocks for v in b['on_kernel_ns']]
    return {'case_id':case_id,'blocks':blocks,'median_block_on_over_off':ratio,'reduction_percent':100*(1-ratio),'off_timed_kernel_ns_sum':sum(off),'on_timed_kernel_ns_sum':sum(on),'off_individual_kernel_ns_median':statistics.median(off),'on_individual_kernel_ns_median':statistics.median(on),'statistical_significance_claim':False}
def quantile(values,q):
    values=sorted(values);c.require(values and 0<=q<=1,'Invalid quantile');position=(len(values)-1)*q;lo=math.floor(position);hi=math.ceil(position)
    return values[lo]+(values[hi]-values[lo])*(position-lo)
def whole_summary(cases,expected_ids):
    c.require(len(cases)==len(expected_ids) and [v['case_id'] for v in cases]==expected_ids and len(set(expected_ids))==len(expected_ids),'Incomplete/duplicate/reordered headline membership')
    ratios=[v['median_block_on_over_off'] for v in cases]
    c.require(all(c.number(v) and v>0 for v in ratios),'Invalid paired ratio')
    geometric=math.exp(math.fsum(math.log(v) for v in ratios)/len(ratios))
    off=sum(v['off_timed_kernel_ns_sum'] for v in cases);on=sum(v['on_timed_kernel_ns_sum'] for v in cases)
    shards=[]
    for index in range(4):
        group=[v for v in cases if int(v['case_id'].split('-')[1])%4==index]
        a=sum(v['off_timed_kernel_ns_sum'] for v in group);b=sum(v['on_timed_kernel_ns_sum'] for v in group)
        shards.append({'shard':index,'cases':len(group),'off_timed_kernel_ns_sum':a,'on_timed_kernel_ns_sum':b,'observed_workload_on_over_off':b/a,'scope':'Same-runner paired timed workload sums; longer OFF cases carry more weight.'})
    top=sorted(cases,key=lambda v:(-v['off_timed_kernel_ns_sum'],v['case_id']))[:5]
    heavy=[{'case_id':v['case_id'],'median_block_on_over_off':v['median_block_on_over_off'],'off_timed_kernel_ns_sum':v['off_timed_kernel_ns_sum'],'on_timed_kernel_ns_sum':v['on_timed_kernel_ns_sum'],'share_of_observed_off_sum':v['off_timed_kernel_ns_sum']/off} for v in top]
    return {'cases':len(cases),'case_equal_weight_geometric_mean_on_over_off':geometric,'geometric_mean_reduction_percent':100*(1-geometric),
      'case_ratio_p50':quantile(ratios,.5),'case_ratio_p90':quantile(ratios,.9),'quantile_method':'linear interpolation at (n-1)*q; p90 ON/OFF describes the slower tail',
      'counts':{'faster':sum(v<1 for v in ratios),'slower':sum(v>1 for v in ratios),'exact_ratio_one':sum(v==1 for v in ratios),'slower_over_1_percent':sum(v>1.01 for v in ratios),'slower_over_5_percent':sum(v>1.05 for v in ratios),'faster_over_1_percent':sum(v<.99 for v in ratios),'faster_over_5_percent':sum(v<.95 for v in ratios)},
      'bands':{'faster_over_5_percent':sum(v<.95 for v in ratios),'faster_1_to_5_percent':sum(.95<=v<.99 for v in ratios),'within_1_percent':sum(.99<=v<=1.01 for v in ratios),'slower_1_to_5_percent':sum(1.01<v<=1.05 for v in ratios),'slower_over_5_percent':sum(v>1.05 for v in ratios)},
      'observed_mixed_runner_workload':{'off_timed_kernel_ns_sum':off,'on_timed_kernel_ns_sum':on,'on_over_off':on/off,'scope':'Auxiliary observed workload across four VMs; not a single-machine time or an equal-case-weight ratio.'},
      'by_shard':shards,'five_heaviest_by_off_sum':heavy,'five_heaviest_share_of_observed_off_sum':sum(v['share_of_observed_off_sum'] for v in heavy),
      'ratio_definition':'For each case, median of three ABBA block sum(ON kernel_ns)/sum(OFF kernel_ns). Not ratio of arm medians.',
      'statistical_significance_claim':False,'adoption_approved':False}
