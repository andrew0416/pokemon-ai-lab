//! P1e4 TEST-ONLY original concrete stage oracle with external-memory distribution storage.
use lab_engine::{action::{JointAction,SlotAction},rules::Ruleset,
    state::{State,SideId},turn::{legal_joint_actions,Suspension,exact_stream_oracle}};
use lab_scenario::{load_scenario_file,scenario_positions,LoadedScenario,Position};
use serde_json::{json,Value};
use std::path::{Path,PathBuf};
const EPS:f64=1e-9;
type Key=(State<2>,Option<Suspension>);
#[derive(Clone)]
struct SplitMix64(u64);
impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }
    fn index(&mut self, n: usize) -> Result<usize, String> {
        if n == 0 {
            return Err("empty action support".into());
        }
        let n = u64::try_from(n).map_err(|_| "index bound exceeds u64")?;
        let threshold = n.wrapping_neg() % n;
        loop {
            let x = self.next();
            if x >= threshold {
                return Ok((x % n) as usize);
            }
        }
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 * (1.0 / 9007199254740992.0)
    }
}

fn positive(v: f64, name: &str) -> Result<(), String> {
    if !v.is_finite() || v <= 0.0 {
        Err(format!("{name} is not finite positive: {v}"))
    } else {
        Ok(())
    }
}
fn unit_mass(v: f64, name: &str) -> Result<(), String> {
    positive(v, name)?;
    if (v - 1.0).abs() > EPS {
        Err(format!("{name} does not sum to one: {v}"))
    } else {
        Ok(())
    }
}
fn bounded(v: f64, name: &str) -> Result<f64, String> {
    if !v.is_finite() || v < -EPS || v > 1.0 + EPS {
        Err(format!("{name} outside probability bounds: {v}"))
    } else {
        Ok(v.clamp(0.0, 1.0))
    }
}
fn project(key: &Key) -> Key {
    let mut out = key.clone();
    for side in &mut out.0.sides {
        for mon in &mut side.party {
            mon.hp = 0;
        }
    }
    out
}
fn restore_checked(state: &State<2>, original: &State<2>, stage: &str) -> Result<(), String> {
    if state != original {
        Err(format!("State was not restored after {stage}"))
    } else {
        Ok(())
    }
}

struct Selection {
    state: State<2>,
    choices: [JointAction<2>; 2],
    description: Value,
}
fn describe(loaded: &LoadedScenario, seed: u64) -> Result<Selection, String> {
    if !loaded.setup_turns.is_empty()
        || loaded.patch.is_some()
        || loaded.start_state.is_some()
        || !loaded.setup_states.is_empty()
        || loaded.setup_rolls.is_some()
        || loaded.mid_turn.iter().any(|v| !v.is_empty())
    {
        return Err(
            "opening input may not contain setup, patch, filtered starts or midTurn replay".into(),
        );
    }
    let positions = scenario_positions(loaded).map_err(|e| format!("opening setup: {e}"))?;
    select_position(positions, seed)
}
fn select_position(positions: Vec<Position>, seed: u64) -> Result<Selection, String> {
    if positions.is_empty() {
        return Err("opening support is empty".into());
    }
    let mut ordered: Vec<_> = positions
        .into_iter()
        .map(|p| {
            let sort = (
                format!("{:?}", p.state),
                format!("{:?}", p.order),
                p.probability.to_bits(),
            );
            (sort, p)
        })
        .collect();
    ordered.sort_by(|a, b| a.0.cmp(&b.0));
    let mut total = 0.0;
    for (_, p) in &ordered {
        positive(p.probability, "opening probability")?;
        total += p.probability;
    }
    unit_mass(total, "opening probabilities")?;
    let mut rng = SplitMix64(seed);
    let target = rng.unit() * total;
    let mut cumulative = 0.0;
    let mut chosen = ordered.len() - 1;
    for (i, (_, p)) in ordered.iter().enumerate() {
        cumulative += p.probability;
        if target < cumulative {
            chosen = i;
            break;
        }
    }
    let position = &ordered[chosen].1;
    let mut choices = [[SlotAction::Pass; 2]; 2];
    let mut counts = [0usize; 2];
    let mut indices = [0usize; 2];
    for side in [SideId::One, SideId::Two] {
        let actions: Vec<_> = legal_joint_actions(&position.state, Ruleset::CHAMPIONS_MC, side)
            .into_iter()
            .filter(|joint| joint.iter().all(|a| matches!(a, SlotAction::Move { .. })))
            .collect();
        let i = side.index();
        counts[i] = actions.len();
        indices[i] = rng.index(actions.len())?;
        choices[i] = actions[indices[i]];
    }
    let action_json: Vec<Vec<Value>> = choices
        .iter()
        .map(|side| {
            side.iter()
                .map(|action| match action {
                    SlotAction::Move {
                        index,
                        target,
                        gimmick,
                    } => {
                        json!({"move_index":index,"target":target,"gimmick":format!("{gimmick:?}")})
                    }
                    _ => unreachable!("move-only selection"),
                })
                .collect()
        })
        .collect();
    let description = json!({"schema":1,"kind":"frozen-opening-selection","joint_seed":seed,
        "rng":"splitmix64-v1; weighted opening uses high53-bit uniform; joint index uses rejection",
        "position_order":"State Debug, party-order Debug, probability bits; ascending",
        "joint_policy":"uniform legal move-only joint actions per side; target and legal Mega variants count separately; no switch/pass",
        "ruleset":"CHAMPIONS_MC","position_count":ordered.len(),"position_index":chosen,
        "position_probability":position.probability,"position_probability_bits":position.probability.to_bits(),
        "position_total_mass":total,"eligible_joint_counts":counts,"joint_indices":indices,
        "full_state_debug":format!("{:?}",position.state),"party_order_debug":format!("{:?}",position.order),
        "choices_debug":format!("{choices:?}"),"choices":action_json});
    Ok(Selection {
        state: position.state.clone(),
        choices,
        description,
    })
}
fn description_bytes(value: &Value) -> Result<Vec<u8>, String> {
    let mut bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    Ok(bytes)
}
fn check_plan(description: &Value, path: &Path) -> Result<(), String> {
    let supplied = std::fs::read(path).map_err(|e| format!("plan read: {e}"))?;
    check_plan_bytes(description, &supplied)
}
fn check_plan_bytes(description: &Value, supplied: &[u8]) -> Result<(), String> {
    if supplied != description_bytes(description)? {
        return Err("frozen description bytes do not match reproduced state/actions".into());
    }
    Ok(())
}



// Concrete stream export is deliberately a different method from the P1e2 factored exporter.
fn write_new(path:&Path,bytes:&[u8])->Result<(),String> {
    use std::io::Write;
    let mut file=std::fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|e|e.to_string())?;
    file.write_all(bytes).map_err(|e|e.to_string())?;file.sync_all().map_err(|e|e.to_string())
}
fn run()->Result<(),String> {
    let mut args=std::env::args().skip(1);
    let scenario=PathBuf::from(args.next().ok_or("scenario path required")?);
    let mut seed=None;let mut plan=None;let mut out=None;let mut scratch=None;let mut describe_only=false;
    while let Some(flag)=args.next() {
        match flag.as_str() {
            "--joint-seed"=>seed=Some(args.next().ok_or("missing seed")?.parse::<u64>().map_err(|e|e.to_string())?),
            "--plan"=>plan=Some(PathBuf::from(args.next().ok_or("missing plan")?)),
            "--out"=>out=Some(PathBuf::from(args.next().ok_or("missing output")?)),
            "--scratch"=>scratch=Some(PathBuf::from(args.next().ok_or("missing scratch")?)),
            "--describe"=>describe_only=true,
            _=>return Err(format!("unknown argument {flag}")),
        }
    }
    let loaded=load_scenario_file(&scenario).map_err(|e|format!("load: {e}"))?;
    let selected=describe(&loaded,seed.ok_or("--joint-seed required")?)?;
    if let Some(ref p)=plan {check_plan(&selected.description,p)?;}
    let selection=description_bytes(&selected.description)?;
    if describe_only {
        use std::io::Write;
        return std::io::stdout().write_all(&selection).map_err(|e|e.to_string());
    }
    let _plan=plan.ok_or("--plan required for execution")?;
    let out=out.ok_or("--out required")?;let scratch=scratch.ok_or("--scratch required")?;
    if out.exists() || scratch.exists() {return Err("output and scratch must both be new paths".into());}
    std::fs::create_dir(&out).map_err(|e|e.to_string())?;
    let limits=exact_stream_oracle::Limits::default();
    let started=std::time::Instant::now();
    let exported=exact_stream_oracle::export_turn(&selected.state,Ruleset::CHAMPIONS_MC,
        selected.choices,&scratch,&out.join("joint.bin"),limits.clone())?;
    let oracle_ns=started.elapsed().as_nanos();
    let dictionary=json!({"schema":1,"kind":"full-non-hp-state-suspension-dictionary",
        "party_lengths":[6,6],"entries":exported.dictionary});
    let mut dictionary_bytes=serde_json::to_vec(&dictionary).map_err(|e|e.to_string())?;dictionary_bytes.push(b'\n');
    let joint_bytes=std::fs::metadata(out.join("joint.bin")).map_err(|e|e.to_string())?.len();
    if joint_bytes!=exported.rows*36 {return Err("joint byte count mismatch".into());}
    let stages:Vec<Value>=exported.stages.iter().map(|s|json!({
        "stage":s.stage,"input_rows":s.input_rows,"input_dictionary_entries":s.input_dictionary_entries,
        "replays":s.replays,"next_rows":s.next_rows,"next_dictionary_entries":s.next_dictionary_entries,
        "finished_dictionary_entries":s.finished_dictionary_entries,"active_mass":s.active_mass,"finished_mass":s.finished_mass
    })).collect();
    let manifest=json!({
        "schema":1,"kind":"exact-stream-oracle-export","status":"complete",
        "method":"original concrete run_stage + Full Chooser replay; external-memory exact joint-key stage merging",
        "metric_schema":"full State plus full Suspension; only all current Pokemon.hp values projected and restored by joint tuple",
        "dictionary":"dictionary.json","joint":"joint.bin","selection":"selection.json",
        "party_lengths":[6,6],"hp_unit_count":12,
        "hp_unit_order":"SideId::One party index ascending, then SideId::Two party index ascending; all members including inactive, fainted and empty",
        "record_bytes":36,"record_layout":"little-endian u32 dictionary_id; hp_unit_count signed i16 HP; IEEE754 binary64 probability; no header or padding",
        "record_order":"dictionary_id ascending then signed HP tuple lexicographic; duplicate tuples summed",
        "probability_policy":"raw original chance probabilities; no renormalization, pruning, clamping, candidate filtering or HP independence; compensated duplicate sums",
        "dictionary_entries":dictionary["entries"].as_array().unwrap().len(),
        "suspended_dictionary_entries":exported.suspended_dictionary_entries,
        "unique_joint_rows":exported.rows,"joint_mass":exported.mass,"tv_bound":0.0,
        "joint_bytes":joint_bytes,"dictionary_bytes":dictionary_bytes.len(),"selection_bytes":selection.len(),
        "payload_bytes_excluding_manifest":joint_bytes+dictionary_bytes.len() as u64+selection.len() as u64,
        "max_export_bytes_including_manifest":536870912u64,
        "limits":{"chunk_rows":limits.chunk_rows,"merge_fan_in":limits.merge_fan_in,
            "dictionary_entries_per_live_dictionary":limits.dictionary_entries,
            "dictionary_debug_bytes_per_live_dictionary":limits.dictionary_debug_bytes,
            "scratch_bytes_cumulative":limits.scratch_bytes,"max_unique_rows":limits.max_unique_rows,
            "max_replays":limits.max_replays,"max_instruction_rows":limits.max_instruction_rows,"max_stages":limits.max_stages},
        "scratch_bytes_written":exported.scratch_bytes_written,"total_replays":exported.replays,"instruction_rows":exported.instruction_rows,
        "stage_reports":stages,"caller_state_immutable":true,"all_state_restored":true,
        "all_lazy_tags_clear":true,"actual_eq_hash_dictionary":true,"debug_injective_on_observed_keys":true,
        "suspension_scope":"first mid-turn pause, no automatic resume; complete pending state included in key",
        "oracle_ns":oracle_ns,
        "timing_scope":"diagnostic concrete oracle computation including external sort and disk I/O; not a performance benchmark",
        "adoption_approved":false,"full500_complete":false
    });
    let mut manifest_bytes=serde_json::to_vec(&manifest).map_err(|e|e.to_string())?;manifest_bytes.push(b'\n');
    let total=joint_bytes+dictionary_bytes.len() as u64+selection.len() as u64+manifest_bytes.len() as u64;
    if total>536870912 {return Err("complete export byte limit".into());}
    write_new(&out.join("dictionary.json"),&dictionary_bytes)?;
    write_new(&out.join("selection.json"),&selection)?;
    // Completion marker is exclusive and last. Partial output is never an oracle.
    write_new(&out.join("manifest.json"),&manifest_bytes)?;
    use std::io::Write;
    std::io::stdout().write_all(&manifest_bytes).map_err(|e|e.to_string())
}
fn main() {if let Err(error)=run(){eprintln!("P1E4_INCONCLUSIVE: {error}");std::process::exit(1);}}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU64,Ordering};
    use lab_engine::{instruction::Outcome,turn::{enumerate_turn_with,EnumerateOptions,FactoredScope,RollMode}};
    use lab_scenario::{scenario_decision,Decision};
    static NEXT:AtomicU64=AtomicU64::new(0);
    type Dist=BTreeMap<(String,[i16;12]),f64>;
    fn directory()->PathBuf {
        let path=std::env::temp_dir().join(format!("p1e4-bin-{}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::SeqCst)));
        std::fs::create_dir(&path).unwrap();path
    }
    fn baseline(state:&State<2>,outcomes:Vec<Outcome>)->Dist {
        let mut out=Dist::new();
        for outcome in outcomes {
            let mut end=state.clone();end.apply(&outcome.instructions);
            let mut hp=[0;12];
            for (i,mon) in end.sides.iter().flat_map(|s|s.party.iter()).enumerate(){hp[i]=mon.hp;}
            let key=project(&(end.clone(),outcome.suspension));
            let text=format!("{:?}\n{:?}",key.0,key.1);
            *out.entry((text,hp)).or_default()+=outcome.probability;
            end.reverse(&outcome.instructions);assert_eq!(&end,state);
        }
        out
    }
    fn streamed(path:&Path,entries:&[String])->Dist {
        use std::io::Read;
        let mut f=std::fs::File::open(path).unwrap();let mut raw=Vec::new();f.read_to_end(&mut raw).unwrap();
        assert_eq!(raw.len()%36,0);let mut out=Dist::new();
        for r in raw.chunks_exact(36) {
            let id=u32::from_le_bytes(r[..4].try_into().unwrap()) as usize;let mut hp=[0;12];
            for (i,h) in hp.iter_mut().enumerate(){*h=i16::from_le_bytes(r[4+2*i..6+2*i].try_into().unwrap());}
            let p=f64::from_le_bytes(r[28..36].try_into().unwrap());
            assert!(out.insert((entries[id].clone(),hp),p).is_none());
        }
        out
    }
    fn same(a:&Dist,b:&Dist) {
        assert_eq!(a.keys().collect::<Vec<_>>(),b.keys().collect::<Vec<_>>());
        for (k,p) in a {assert!((p-b[k]).abs()<=1e-12);}
        assert!((a.values().sum::<f64>()-1.0).abs()<=1e-9);
        assert!((b.values().sum::<f64>()-1.0).abs()<=1e-9);
    }
    fn compare_fixture(name:&str,expect_pause:bool) {
        let loaded=load_scenario_file(Path::new(env!("CARGO_MANIFEST_DIR")).join("../oracle/scenarios").join(format!("{name}.json"))).unwrap();
        let positions=scenario_positions(&loaded).unwrap();assert!(!positions.is_empty());
        let pos=&positions[0];let Decision::Turn(choices)=scenario_decision(&loaded,pos).unwrap() else {panic!("turn fixture")};
        let state=pos.state.clone();let original=state.clone();
        let flat={let _scope=FactoredScope::new(false);let mut work=state.clone();
            let values=enumerate_turn_with(&mut work,Ruleset::CHAMPIONS_MC,choices,EnumerateOptions{rolls:RollMode::Full}).unwrap();
            assert_eq!(work,state);baseline(&state,values)};
        let factored={let _scope=FactoredScope::new(true);let mut work=state.clone();
            let values=enumerate_turn_with(&mut work,Ruleset::CHAMPIONS_MC,choices,EnumerateOptions{rolls:RollMode::Full}).unwrap();
            assert_eq!(work,state);baseline(&state,values)};
        same(&flat,&factored);
        let dir=directory();let limits=exact_stream_oracle::Limits{chunk_rows:3,merge_fan_in:2,..Default::default()};
        let report=exact_stream_oracle::export_turn(&state,Ruleset::CHAMPIONS_MC,choices,&dir.join("scratch"),&dir.join("joint.bin"),limits).unwrap();
        assert_eq!(state,original);same(&flat,&streamed(&dir.join("joint.bin"),&report.dictionary));
        assert_eq!(report.suspended_dictionary_entries>0,expect_pause);
    }
    #[test]
    fn concrete_stream_matches_original_flat_and_factored(){compare_fixture("single-hit",false);}
    #[test]
    fn concrete_stream_preserves_real_full_suspension(){compare_fixture("counter-uturn",true);}
    #[test]
    fn frozen_plan_requires_exact_bytes(){
        let desc=json!({"test":1});let bytes=description_bytes(&desc).unwrap();
        check_plan_bytes(&desc,&bytes).unwrap();assert!(check_plan_bytes(&desc,b"{ \"test\":1}\n").is_err());
    }
    #[test]
    fn resource_failure_after_real_stage_keeps_input_and_has_no_manifest(){
        let loaded=load_scenario_file(Path::new(env!("CARGO_MANIFEST_DIR")).join("../oracle/scenarios/single-hit.json")).unwrap();
        let positions=scenario_positions(&loaded).unwrap();let pos=&positions[0];
        let Decision::Turn(choices)=scenario_decision(&loaded,pos).unwrap() else {panic!("turn fixture")};
        let state=pos.state.clone();let before=format!("{state:?}");let dir=directory();
        let limits=exact_stream_oracle::Limits{max_instruction_rows:0,..Default::default()};
        let error=exact_stream_oracle::export_turn(&state,Ruleset::CHAMPIONS_MC,choices,&dir.join("scratch"),&dir.join("joint.bin"),limits).unwrap_err();
        assert!(error.contains("emission instruction-row resource limit"));
        assert!(error.contains("rollback verified"));
        assert_eq!(format!("{state:?}"),before);
        assert!(!dir.join("manifest.json").exists());assert!(!dir.join("joint.bin").exists());
        // A subsequent concrete invocation on the same thread is still usable.
        let retry=directory();let report=exact_stream_oracle::export_turn(&state,Ruleset::CHAMPIONS_MC,choices,&retry.join("scratch"),&retry.join("joint.bin"),Default::default()).unwrap();
        assert!(report.rows>0);assert_eq!(format!("{state:?}"),before);
    }

}
