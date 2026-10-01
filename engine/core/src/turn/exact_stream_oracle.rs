//! TEST-ONLY concrete exact oracle. Existing battle/Chooser/lazy code is unchanged.
//! Compact joint HP rows replace the all-in-memory staged Merger. No HP independence,
//! candidate support, fixed-roll approximation, normalization or pruning is used.
use super::{check_turn, initial_queue, run_stage, Pending, StageEnd, Suspension};
use super::battle::{Battle, RunBuffers, RunStart};
use super::branch::{Chooser, RollMode};
use crate::action::JointAction;
use crate::instruction::Instruction;
use crate::rules::Ruleset;
use crate::state::{PokemonRef, SideId, State, PARTY_SIZE};
use std::{
    cmp::Reverse, collections::{BinaryHeap, HashMap}, fmt::{self, Write as FmtWrite},
    fs::{File, OpenOptions}, hash::{Hash, Hasher},
    io::{BufReader, BufWriter, Read, Write}, path::{Path, PathBuf},
};

const UNITS: usize = PARTY_SIZE * 2;
pub const ROW_BYTES: u64 = 36;
const EPS: f64 = 1e-9;
type Result<T> = std::result::Result<T, String>;
#[derive(Clone, Debug)]
pub struct Limits {
    pub chunk_rows: usize,
    pub merge_fan_in: usize,
    pub dictionary_entries: usize,
    pub dictionary_debug_bytes: usize,
    pub scratch_bytes: u64,
    pub max_unique_rows: u64,
    pub max_replays: u64,
    pub max_instruction_rows: u64,
    pub max_stages: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self { chunk_rows: 65_536, merge_fan_in: 16, dictionary_entries: 4096,
            dictionary_debug_bytes: 134_217_728, scratch_bytes: 4_294_967_296,
            max_unique_rows: 10_000_000, max_replays: 1_000_000_000, max_instruction_rows: 10_000_000_000, max_stages: 64 }
    }
}
#[derive(Clone, Debug)]
pub struct StageReport {
    pub stage: usize, pub input_rows: u64, pub input_dictionary_entries: usize,
    pub replays: u64, pub next_rows: u64, pub next_dictionary_entries: usize,
    pub finished_dictionary_entries: usize, pub active_mass: f64, pub finished_mass: f64,
}
#[derive(Debug)]
pub struct Export {
    pub dictionary: Vec<String>,
    pub rows: u64, pub mass: f64, pub suspended_dictionary_entries: usize,
    pub scratch_bytes_written: u64, pub replays: u64, pub instruction_rows: u64, pub stages: Vec<StageReport>,
}
#[derive(Clone, Copy, Debug, Default)]
struct Sum { value: f64, correction: f64 }
impl Sum {
    fn add(&mut self, x: f64) {
        let t=self.value+x;
        self.correction += if self.value.abs()>=x.abs() {(self.value-t)+x} else {(x-t)+self.value};
        self.value=t;
    }
    fn total(self)->f64 { self.value+self.correction }
}
fn require(ok: bool, why: &str)->Result<()> { if ok {Ok(())} else {Err(why.into())} }
fn positive(p:f64)->Result<()> { require(p.is_finite() && p>0.0,"nonpositive/nonfinite raw mass") }
fn unit_mass(p:f64)->Result<()> { positive(p)?;require((p-1.0).abs()<=EPS,"raw probability mass is not one") }
fn io<T>(value:std::io::Result<T>)->Result<T> { value.map_err(|e|e.to_string()) }
fn hash(v:&impl Hash)->u64 {
    let mut h=std::collections::hash_map::DefaultHasher::new();v.hash(&mut h);h.finish()
}
fn unit(i:usize)->PokemonRef {
    PokemonRef { side:if i<PARTY_SIZE {SideId::One} else {SideId::Two}, party:(i%PARTY_SIZE) as u8 }
}
fn clear_tags(state:&State<2>)->Result<()> {
    require(state.sides.iter().flat_map(|s|s.party.iter()).all(|p|p.lazy.0==0),"lazy tag in concrete oracle")
}
fn hps(state:&State<2>)->Result<[i16;UNITS]> {
    clear_tags(state)?;
    let mut out=[0;UNITS];
    for (i,hp) in out.iter_mut().enumerate() {
        let mon=state.pokemon(unit(i));*hp=mon.hp_value();
        require(*hp>=0 && *hp<=mon.max_hp,"HP outside concrete bounds")?;
    }
    Ok(out)
}
fn project(state:&State<2>, values:&[i16;UNITS])->State<2> {
    let mut key=state.clone();
    for (i,&amount) in values.iter().enumerate() {
        key.apply_one(&Instruction::Damage {target:unit(i),amount});
    }
    key
}
fn restore(state:&State<2>, values:&[i16;UNITS])->Result<State<2>> {
    require(hps(state)?==[0;UNITS],"dictionary is not zero-HP projected")?;
    let mut out=state.clone();
    for (i,&amount) in values.iter().enumerate() {
        require(amount>=0 && amount<=out.pokemon(unit(i)).max_hp,"bad stored HP")?;
        out.apply_one(&Instruction::Heal {target:unit(i),amount});
    }
    require(hps(&out)?==*values,"HP reconstruction mismatch")?;
    Ok(out)
}
struct LimitedText { text:String, limit:usize }
impl fmt::Write for LimitedText {
    fn write_str(&mut self,s:&str)->fmt::Result {
        if self.text.len().checked_add(s.len()).is_none_or(|n|n>self.limit) {return Err(fmt::Error);}
        self.text.push_str(s);Ok(())
    }
}
struct Entry<P> { state:State<2>, rest:P, debug:String }
struct Dictionary<P> { entries:Vec<Entry<P>>, index:HashMap<u64,Vec<u32>>, debug_index:HashMap<u64,Vec<u32>>, bytes:usize }
impl<P:Clone+Eq+Hash+fmt::Debug> Dictionary<P> {
    fn new()->Self {Self {entries:Vec::new(),index:HashMap::new(),debug_index:HashMap::new(),bytes:0}}
    fn intern(&mut self,state:&State<2>,rest:&P,limits:&Limits)->Result<Key> {
        let values=hps(state)?;
        let projected=project(state,&values);
        // Only current Pokemon HP is projected. Everything in P stays concrete and intact.
        let fingerprint=hash(&(&projected,rest));
        if let Some(ids)=self.index.get(&fingerprint) {
            for &id in ids {
                let entry=&self.entries[id as usize];
                if entry.state==projected && entry.rest==*rest {
                    return Ok(Key{id,hps:values});
                }
            }
        }
        require(self.entries.len()<limits.dictionary_entries,"non-HP dictionary entry limit")?;
        require(self.entries.len()<u32::MAX as usize,"dictionary ID overflow")?;
        // Eq/Hash determines identity first; Debug only checks serialization injectivity.
        // LazyTag is the intentionally ignored State field and was checked explicitly above.
        let mut text=LimitedText{text:String::new(),limit:limits.dictionary_debug_bytes.saturating_sub(self.bytes)};
        write!(&mut text,"{:?}\n{:?}",projected,rest).map_err(|_|"non-HP dictionary Debug byte limit")?;
        let debug_hash=hash(&text.text);
        if let Some(ids)=self.debug_index.get(&debug_hash) {
            require(ids.iter().all(|&i|self.entries[i as usize].debug!=text.text),"unequal Eq/Hash keys have the same Debug")?;
        }
        require(restore(&projected,&values)?==*state,"full State projection/reconstruction mismatch")?;
        let id=self.entries.len() as u32;
        self.bytes+=text.text.len();
        self.entries.push(Entry{state:projected,rest:rest.clone(),debug:text.text});
        self.index.entry(fingerprint).or_default().push(id);
        self.debug_index.entry(debug_hash).or_default().push(id);
        Ok(Key{id,hps:values})
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Key {id:u32,hps:[i16;UNITS]}
#[derive(Clone, Copy, Debug)]
struct Row {key:Key,p:f64}
fn write_row(w:&mut impl Write,row:Row)->Result<()> {
    positive(row.p)?;
    require(row.key.hps.iter().all(|&h|h>=0),"negative joint HP")?;
    let mut raw=[0u8;ROW_BYTES as usize];
    raw[..4].copy_from_slice(&row.key.id.to_le_bytes());
    for (i,h) in row.key.hps.iter().enumerate() {raw[4+2*i..6+2*i].copy_from_slice(&h.to_le_bytes());}
    raw[28..36].copy_from_slice(&row.p.to_le_bytes());io(w.write_all(&raw))
}
fn read_row(r:&mut impl Read)->Result<Option<Row>> {
    let mut raw=[0u8;ROW_BYTES as usize];
    if io(r.read(&mut raw[..1]))?==0 {return Ok(None);}
    io(r.read_exact(&mut raw[1..]))?;
    let id=u32::from_le_bytes(raw[..4].try_into().unwrap());
    let mut hp=[0;UNITS];
    for (i,h) in hp.iter_mut().enumerate() {*h=i16::from_le_bytes(raw[4+2*i..6+2*i].try_into().unwrap());}
    let p=f64::from_le_bytes(raw[28..36].try_into().unwrap());positive(p)?;
    require(hp.iter().all(|&h|h>=0),"negative stored HP")?;
    Ok(Some(Row{key:Key{id,hps:hp},p}))
}
struct Ledger {bytes:u64,limit:u64}
impl Ledger {
    fn reserve_row(&mut self)->Result<()> {
        self.bytes=self.bytes.checked_add(ROW_BYTES).ok_or("scratch byte overflow")?;
        require(self.bytes<=self.limit,"cumulative scratch byte limit")
    }
}
#[derive(Debug)]
struct Spool {path:PathBuf,rows:u64,mass:f64}
struct Sorter {
    root:PathBuf,label:String,buffer:Vec<Row>,runs:Vec<PathBuf>,serial:u64,limits:Limits,
}
impl Sorter {
    fn new(root:&Path,label:&str,limits:&Limits)->Self {
        Self{root:root.into(),label:label.into(),buffer:Vec::with_capacity(limits.chunk_rows),
            runs:Vec::new(),serial:0,limits:limits.clone()}
    }
    fn path(&mut self)->PathBuf {
        let p=self.root.join(format!("{}-{:06}.bin",self.label,self.serial));self.serial+=1;p
    }
    fn push(&mut self,row:Row,ledger:&mut Ledger)->Result<()> {
        positive(row.p)?;self.buffer.push(row);
        if self.buffer.len()>=self.limits.chunk_rows {self.flush(ledger)?;} Ok(())
    }
    fn flush(&mut self,ledger:&mut Ledger)->Result<()> {
        if self.buffer.is_empty(){return Ok(());}
        self.buffer.sort_by_key(|r|r.key);
        let path=self.path();let mut writer=BufWriter::new(io(OpenOptions::new().write(true).create_new(true).open(&path))?);
        let mut at=0;
        while at<self.buffer.len() {
            let key=self.buffer[at].key;let mut sum=Sum::default();
            while at<self.buffer.len() && self.buffer[at].key==key {sum.add(self.buffer[at].p);at+=1;}
            ledger.reserve_row()?;write_row(&mut writer,Row{key,p:sum.total()})?;
        }
        io(writer.flush())?;self.buffer.clear();self.runs.push(path);Ok(())
    }
    fn merge(&mut self,paths:&[PathBuf],ledger:&mut Ledger)->Result<Spool> {
        require(paths.len()<=self.limits.merge_fan_in,"merge fan-in limit")?;
        let path=self.path();let mut writer=BufWriter::new(io(OpenOptions::new().write(true).create_new(true).open(&path))?);
        let mut readers=paths.iter().map(|p|io(File::open(p)).map(BufReader::new)).collect::<Result<Vec<_>>>()?;
        let mut current=Vec::with_capacity(readers.len());let mut heap=BinaryHeap::new();
        for (i,r) in readers.iter_mut().enumerate() {
            let row=read_row(r)?;if let Some(row)=row {heap.push(Reverse((row.key,i)));}current.push(row);
        }
        let mut pending:Option<Key>=None;let mut sum=Sum::default();let mut mass=Sum::default();let mut count=0;
        while let Some(Reverse((key,i)))=heap.pop() {
            if pending.is_some_and(|p|p!=key) {
                let p=sum.total();ledger.reserve_row()?;write_row(&mut writer,Row{key:pending.unwrap(),p})?;
                mass.add(p);count+=1;require(count<=self.limits.max_unique_rows,"unique rows exceed limit")?;
                sum=Sum::default();
            }
            pending=Some(key);sum.add(current[i].take().expect("heap row").p);
            let next=read_row(&mut readers[i])?;
            if let Some(row)=next {
                require(key<row.key,"unsorted or duplicate scratch run")?;
                heap.push(Reverse((row.key,i)));
            }
            current[i]=next;
        }
        if let Some(key)=pending {
            let p=sum.total();ledger.reserve_row()?;write_row(&mut writer,Row{key,p})?;mass.add(p);count+=1;
            require(count<=self.limits.max_unique_rows,"unique rows exceed limit")?;
        }
        io(writer.flush())?;
        Ok(Spool{path,rows:count,mass:mass.total()})
    }
    fn finish(mut self,ledger:&mut Ledger)->Result<Spool> {
        self.flush(ledger)?;
        let mut runs=std::mem::take(&mut self.runs);
        while runs.len()>self.limits.merge_fan_in {
            let mut next=Vec::new();
            for group in runs.chunks(self.limits.merge_fan_in) {next.push(self.merge(group,ledger)?.path);}
            runs=next;
        }
        self.merge(&runs,ledger)
    }
}
fn rollback_checked(work:&mut State<2>,before:&State<2>,buffers:&RunBuffers,result:Result<()>)->Result<()> {
    let changed=*work!=*before;
    work.reverse(&buffers.log);
    require(*work==*before,"stage instruction rollback differs")?;
    clear_tags(work)?;
    if let Err(error)=result {
        super::lazy::end();
        eprintln!("P1E4_REPLAY_FAILED instructions={} changed_before_rollback={} rollback_verified=true tls_cleared=true",buffers.log.len(),changed);
        return Err(format!("after-stage emission failed (instructions={}, rollback verified): {error}",buffers.log.len()));
    }
    Ok(())
}
struct NoLazy;
impl Drop for NoLazy { fn drop(&mut self){super::lazy::end();} }

/// Concrete original stage kernel with a bounded external-memory distribution store.
/// Inputs are immutable; output is the complete first-pause distribution, not auto-resumed.
/// The caller writes a completed manifest only after this returns and its own checks pass.
pub fn export_turn(state:&State<2>,ruleset:Ruleset,choices:[JointAction<2>;2],
                   scratch:&Path,joint_output:&Path,limits:Limits)->Result<Export> {
    require(limits.chunk_rows>0 && limits.merge_fan_in>=2 && limits.dictionary_entries>0
        && limits.dictionary_debug_bytes>0 && limits.max_unique_rows>0 && limits.max_stages>0,
        "invalid oracle limits")?;
    require(!scratch.exists() && !joint_output.exists(),"oracle paths must be new")?;
    clear_tags(state)?;
    let checked=check_turn(state,ruleset,&choices).map_err(|e|format!("check_turn: {e:?}"))?;
    io(std::fs::create_dir(scratch))?;
    super::lazy::end();let _no_lazy=NoLazy;
    let mut ledger=Ledger{bytes:0,limit:limits.scratch_bytes};
    let mut current=Dictionary::<Pending>::new();
    let first=current.intern(state,&Pending::new(initial_queue(state,&checked)),&limits)?;
    let mut seed=Sorter::new(scratch,"initial",&limits);
    seed.push(Row{key:first,p:1.0},&mut ledger)?;
    let mut input=seed.finish(&mut ledger)?;
    let mut finished=Dictionary::<Option<Suspension>>::new();
    let mut final_rows=Sorter::new(scratch,"finished",&limits);
    let mut final_mass=Sum::default();
    let mut buffers=RunBuffers::default();let mut total_replays=0u64;let mut instruction_rows=0u64;let mut reports=Vec::new();
    for stage_index in 0..limits.max_stages {
        if input.rows==0 {break;}
        eprintln!("P1E4_STAGE_BEGIN stage={stage_index} input_rows={} dictionary_entries={} finished_dictionary_entries={} scratch_bytes={}",input.rows,current.entries.len(),finished.entries.len(),ledger.bytes);
        let mut next=Dictionary::<Pending>::new();
        let mut next_rows=Sorter::new(scratch,&format!("stage-{stage_index:03}"),&limits);
        let mut reader=BufReader::new(io(File::open(&input.path))?);
        let mut rows_seen=0u64;let mut input_mass=Sum::default();let mut runs=0u64;
        while let Some(row)=read_row(&mut reader)? {
            rows_seen+=1;input_mass.add(row.p);
            let entry=current.entries.get(row.key.id as usize).ok_or("unknown input dictionary ID")?;
            let mut work=restore(&entry.state,&row.key.hps)?;
            let before=work.clone();let mut after=entry.rest.clone();
            let mut chooser=Chooser::with_rolls(RollMode::Full);let mut start:Option<RunStart>=None;
            let mut conditional_mass=Sum::default();
            #[cfg(feature="experiment-replay-action-keys")]
            let mut action_keys=super::replay_action_keys::ReplayActionKeys::default();
            loop {
                runs+=1;total_replays+=1;
                require(total_replays<=limits.max_replays,"replay count limit")?;
                chooser.begin_run();after.clone_from(&entry.rest);
                let result={
                    let mut battle=match &start {
                        Some(s)=>Battle::replay(&mut work,&mut chooser,s,buffers),
                        None=>{buffers.clear_log();Battle::recycle(&mut work,&mut chooser,buffers)}
                    };
                    if start.is_none(){start=Some(battle.run_start());}
                    #[cfg(feature="experiment-replay-action-keys")]
                    {battle.replay_action_keys=Some(&mut action_keys);}
                    let result=run_stage(&mut battle,&mut after);
                    buffers=battle.into_buffers();result
                };
                // Capture every fallible emission result BEFORE reversing; no ? escapes this scope.
                let emitted=(||->Result<()> {
                    let end=result.map_err(|e|format!("run_stage: {e:?}"))?;
                    instruction_rows=instruction_rows.checked_add(buffers.log.len() as u64).ok_or("instruction row overflow")?;
                    require(instruction_rows<=limits.max_instruction_rows,"emission instruction-row resource limit")?;
                    clear_tags(&work)?;
                    require(super::lazy::take_request().is_none(),"lazy request in concrete oracle")?;
                    let conditional=chooser.probability();positive(conditional)?;
                    conditional_mass.add(conditional);
                    let p=row.p*conditional;positive(p)?;
                    match end {
                        StageEnd::Continue=>{
                            let key=next.intern(&work,&after,&limits)?;
                            next_rows.push(Row{key,p},&mut ledger)?;
                        }
                        StageEnd::Finished|StageEnd::Suspended=>{
                            let kept=(end==StageEnd::Suspended).then(||Suspension(after.clone()));
                            let key=finished.intern(&work,&kept,&limits)?;
                            final_rows.push(Row{key,p},&mut ledger)?;final_mass.add(p);
                        }
                    }
                    Ok(())
                })();
                rollback_checked(&mut work,&before,&buffers,emitted)?;
                if !chooser.advance(){break;}
            }
            unit_mass(conditional_mass.total())?;
        }
        require(rows_seen==input.rows,"input spool row count mismatch")?;
        require((input_mass.total()-input.mass).abs()<=EPS,"input spool mass mismatch")?;
        let next_spool=next_rows.finish(&mut ledger)?;
        final_rows.flush(&mut ledger)?;
        unit_mass(next_spool.mass+final_mass.total())?;
        reports.push(StageReport{stage:stage_index,input_rows:input.rows,input_dictionary_entries:current.entries.len(),
            replays:runs,next_rows:next_spool.rows,next_dictionary_entries:next.entries.len(),
            finished_dictionary_entries:finished.entries.len(),active_mass:next_spool.mass,finished_mass:final_mass.total()});
        eprintln!("P1E4_STAGE_END stage={stage_index} replays={runs} next_rows={} next_dictionary_entries={} finished_dictionary_entries={} active_mass={:.17e} finished_mass={:.17e} scratch_bytes={}",next_spool.rows,next.entries.len(),finished.entries.len(),next_spool.mass,final_mass.total(),ledger.bytes);
        current=next;input=next_spool;
    }
    require(input.rows==0,"stage count limit before termination")?;
    unit_mass(final_mass.total())?;
    let stored=final_rows.finish(&mut ledger)?;
    require(stored.rows>0,"empty final distribution")?;unit_mass(stored.mass)?;
    require((stored.mass-final_mass.total()).abs()<=EPS,"final merged mass differs")?;
    let mut sorted:Vec<usize>=(0..finished.entries.len()).collect();
    sorted.sort_by(|&a,&b|finished.entries[a].debug.cmp(&finished.entries[b].debug));
    let mut remap=vec![0u32;sorted.len()];
    for (id,&old) in sorted.iter().enumerate(){remap[old]=id as u32;}
    let dictionary=sorted.iter().map(|&i|finished.entries[i].debug.clone()).collect::<Vec<_>>();
    let suspended_dictionary_entries=finished.entries.iter().filter(|e|e.rest.is_some()).count();
    let mut ordered=Sorter::new(scratch,"canonical-final",&limits);
    let mut reader=BufReader::new(io(File::open(&stored.path))?);
    while let Some(mut row)=read_row(&mut reader)? {
        let entry=finished.entries.get(row.key.id as usize).ok_or("unknown final dictionary ID")?;
        restore(&entry.state,&row.key.hps)?;
        row.key.id=remap[row.key.id as usize];ordered.push(row,&mut ledger)?;
    }
    let canonical=ordered.finish(&mut ledger)?;
    require(canonical.rows==stored.rows,"canonical ID remap changed support size")?;
    unit_mass(canonical.mass)?;
    let mut src=File::open(&canonical.path).map_err(|e|e.to_string())?;
    let mut dest=io(OpenOptions::new().write(true).create_new(true).open(joint_output))?;
    let copied=io(std::io::copy(&mut src,&mut dest))?;io(dest.sync_all())?;
    require(copied==canonical.rows*ROW_BYTES,"joint output byte count mismatch")?;
    clear_tags(state)?;
    Ok(Export{dictionary,rows:canonical.rows,mass:canonical.mass,suspended_dictionary_entries,
        scratch_bytes_written:ledger.bytes,replays:total_replays,instruction_rows,stages:reports})
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64,Ordering};
    static NEXT:AtomicU64=AtomicU64::new(0);
    fn temp()->PathBuf {
        let p=std::env::temp_dir().join(format!("p1e4-oracle-{}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::SeqCst)));
        std::fs::create_dir(&p).unwrap();p
    }
    fn row(id:u32,h:i16,p:f64)->Row {
        let mut hps=[0;UNITS];hps[0]=h;Row{key:Key{id,hps},p}
    }
    #[test]
    fn external_merge_matches_overlap_and_nonuniform_weights() {
        let dir=temp();let limits=Limits{chunk_rows:2,merge_fan_in:2,..Limits::default()};
        let mut ledger=Ledger{bytes:0,limit:limits.scratch_bytes};
        let mut sorter=Sorter::new(&dir,"overlap",&limits);
        for r in [row(1,3,0.05),row(0,2,0.15),row(1,3,0.20),row(0,1,0.10),
                  row(0,2,0.10),row(1,3,0.10),row(0,1,0.20),row(0,2,0.10)] {sorter.push(r,&mut ledger).unwrap();}
        let spool=sorter.finish(&mut ledger).unwrap();assert_eq!(spool.rows,3);unit_mass(spool.mass).unwrap();
        let mut reader=BufReader::new(File::open(&spool.path).unwrap());let mut actual=Vec::new();
        while let Some(r)=read_row(&mut reader).unwrap(){actual.push(r);}
        assert_eq!(actual.iter().map(|r|r.key).collect::<Vec<_>>(),vec![row(0,1,1.).key,row(0,2,1.).key,row(1,3,1.).key]);
        for (r,p) in actual.iter().zip([0.3,0.35,0.35]){assert!((r.p-p).abs()<1e-15);}
    }
    #[test]
    fn dictionary_roundtrip_preserves_last_reserve_and_pending() {
        let mut state=State::<2>::default();
        state.sides[1].party[5].hp=17;state.sides[1].party[5].max_hp=25;
        let mut dictionary=Dictionary::<Pending>::new();let p=Pending::new(Vec::new());
        let first=dictionary.intern(&state,&p,&Limits::default()).unwrap();
        assert_eq!(first.hps[11],17);
        assert_eq!(restore(&dictionary.entries[0].state,&first.hps).unwrap(),state);
        let mut changed=p.clone();changed.residual_done=true;
        let second=dictionary.intern(&state,&changed,&Limits::default()).unwrap();
        assert_ne!(first.id,second.id);
        state.turn=9;let third=dictionary.intern(&state,&p,&Limits::default()).unwrap();
        assert_ne!(first.id,third.id);
    }
    #[test]
    fn dictionary_rejects_debug_collision_and_resource_limits() {
        #[derive(Clone,PartialEq,Eq,Hash)] struct Hidden(u8);
        impl fmt::Debug for Hidden {fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result{f.write_str("same")}}
        let state=State::<2>::default();let mut dictionary=Dictionary::<Hidden>::new();
        dictionary.intern(&state,&Hidden(0),&Limits::default()).unwrap();
        assert!(dictionary.intern(&state,&Hidden(1),&Limits::default()).is_err());
        let mut dictionary=Dictionary::<u8>::new();let limits=Limits{dictionary_entries:1,..Limits::default()};
        dictionary.intern(&state,&0,&limits).unwrap();assert!(dictionary.intern(&state,&1,&limits).is_err());
        let mut dictionary=Dictionary::<u8>::new();let limits=Limits{dictionary_debug_bytes:1,..Limits::default()};
        assert!(dictionary.intern(&state,&0,&limits).is_err());
    }
    #[test]
    fn wire_rejects_truncation_invalid_mass_and_scratch_limit() {
        let mut raw=Vec::new();write_row(&mut raw,row(0,7,0.25)).unwrap();assert_eq!(raw.len(),36);
        let got=read_row(&mut raw.as_slice()).unwrap().unwrap();assert_eq!(got.key,row(0,7,1.).key);assert_eq!(got.p,0.25);
        assert!(read_row(&mut &raw[..35]).is_err());
        assert!(write_row(&mut Vec::new(),row(0,0,0.)).is_err());
        let dir=temp();let limits=Limits{chunk_rows:1,..Limits::default()};
        let mut sorter=Sorter::new(&dir,"limit",&limits);let mut ledger=Ledger{bytes:0,limit:35};
        assert!(sorter.push(row(0,1,1.),&mut ledger).is_err());
    }
    #[test]
    fn external_rows_keep_correlations_and_last_reserve() {
        let dir=temp();let limits=Limits{chunk_rows:1,merge_fan_in:2,..Limits::default()};
        let mut ledger=Ledger{bytes:0,limit:limits.scratch_bytes};let mut sorter=Sorter::new(&dir,"joint",&limits);
        for h in [10,20] {
            let mut r=row(0,h,0.5);r.key.hps[11]=h;sorter.push(r,&mut ledger).unwrap();
        }
        let spool=sorter.finish(&mut ledger).unwrap();assert_eq!(spool.rows,2);
        let mut reader=BufReader::new(File::open(&spool.path).unwrap());
        while let Some(r)=read_row(&mut reader).unwrap(){assert_eq!(r.key.hps[0],r.key.hps[11]);}
    }
    #[test]
    fn chooser_dynamic_paths_keep_conditional_mass_and_roll_multiplicity() {
        let mut chooser=Chooser::with_rolls(RollMode::Full);let mut paths=Vec::new();
        let mut rolls=[10u16;16];rolls[12..].fill(20);
        loop {
            chooser.begin_run();let hit=chooser.chance(1,4);
            let damage=if hit {chooser.roll(&rolls,SideId::One)} else {0};
            paths.push((damage,chooser.probability()));
            if !chooser.advance(){break;}
        }
        assert_eq!(paths,vec![(10,0.1875),(20,0.0625),(0,0.75)]);
    }

    #[test]
    fn failed_emission_rolls_back_actual_battle_work_and_clears_tls() {
        let mut work=State::<2>::default();work.sides[0].party[0].hp=50;work.sides[0].party[0].max_hp=100;
        let before=work.clone();let mut chooser=Chooser::new();let mut battle=Battle::new(&mut work,&mut chooser);
        battle.apply(Instruction::Damage{target:unit(0),amount:7});
        let buffers=battle.into_buffers();assert!(!buffers.log.is_empty());assert_ne!(work,before);
        super::super::lazy::begin(&[(0,10)]);
        let tagged=crate::state::Pokemon{hp:50,max_hp:100,lazy:super::super::lazy::tag(0),..Default::default()};
        tagged.hp_value();
        let result=rollback_checked(&mut work,&before,&buffers,Err("forced writer failure".into()));
        assert!(result.is_err());assert_eq!(work,before);
        assert_eq!(super::super::lazy::take_request(),None);
    }

}
