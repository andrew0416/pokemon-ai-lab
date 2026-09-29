//! Fixture contracts that cannot be checked by comparing each hidden state separately.
//! Inject as engine/scenario/examples/ci_agreement_oracle.rs. The controller's
//! frozen plan selects the contract before any engine result is observed.
//! Usage: ci_agreement_oracle SCENARIO REPORT --contract CONTRACT --tolerance 1e-9
//! Every branch and probability is preserved; no engine error is an accepted result.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::ExitCode;

use lab_engine::dex::Gender;
use lab_engine::state::{SideId, SlotRef};
use lab_engine::turn::{EnumerateOptions, RollMode};
use lab_scenario::parity::{engine_distribution, report_distribution, Distribution};
use lab_scenario::{
    canonical_value, load_scenario_file, run_decision_mid_turn_with, scenario_decision,
    scenario_positions, LoadedScenario, Position,
};
use serde_json::{json, Value};

type OrderedDistribution = BTreeMap<String, f64>;
const GENDERS: &str = "gender-mixture-v1";
const REDIRECT: &str = "hidden-redirect-order-v1";

fn ordered(distribution: Distribution) -> OrderedDistribution {
    distribution.into_iter().collect()
}

fn mass(distribution: &OrderedDistribution) -> Result<f64, String> {
    if distribution.is_empty() || distribution.values().any(|p| !p.is_finite() || *p < 0.0) {
        return Err("empty distribution or invalid probability".into());
    }
    let total: f64 = distribution.values().sum();
    if (total - 1.0).abs() > 1e-9 {
        return Err(format!("distribution mass is {total}, expected 1"));
    }
    Ok(total)
}

/// Validate raw branch weights before canonical-key merging can hide a negative
/// weight by cancellation. The unit upper bound uses the same 1e-9 rounding
/// allowance as the existing mass check; negative and nonfinite values never pass.
fn raw_probability_mass(probabilities: impl IntoIterator<Item = f64>) -> Result<f64, String> {
    let mut total = 0.0;
    let mut count = 0usize;
    for probability in probabilities {
        if !probability.is_finite() || probability < 0.0 || probability > 1.0 + 1e-9 {
            return Err(format!("invalid raw probability {probability}"));
        }
        count += 1;
        total += probability;
    }
    if count == 0 || !total.is_finite() || (total - 1.0).abs() > 1e-9 {
        return Err(format!("raw probability mass is {total}, expected 1"));
    }
    Ok(total)
}

fn checked_oracle_distribution(report: &Value) -> Result<OrderedDistribution, String> {
    let rows = report["outcomes"].as_array().ok_or("no oracle outcomes")?;
    let probabilities = rows
        .iter()
        .map(|row| row["p"].as_f64().ok_or("invalid oracle probability"))
        .collect::<Result<Vec<_>, _>>()?;
    raw_probability_mass(probabilities)?;
    Ok(ordered(report_distribution(report)?))
}

/// These four reports describe every generated hidden start, not a conditional
/// subset. Check the complete parent mass before examining canonical agreement.
fn validate_contract_parents(
    loaded: &LoadedScenario,
    positions: &[Position],
    before: &Value,
) -> Result<f64, String> {
    let total = raw_probability_mass(positions.iter().map(|position| position.probability))?;
    for position in positions {
        if canonical_value(&position.state, &loaded.meta).map_err(|e| e.to_string())? != *before {
            return Err("contract has a generated parent outside oracle.before".into());
        }
    }
    Ok(total)
}

// The injected probe also runs on historical revisions. Its diagnostics must not
// depend on HashMap iteration order in those revisions' comparator.
fn comparison(engine: &OrderedDistribution, oracle: &OrderedDistribution, tolerance: f64) -> Value {
    let keys: BTreeSet<_> = engine.keys().chain(oracle.keys()).collect();
    let mut only_engine = Vec::new();
    let mut only_oracle = Vec::new();
    let mut maximum = 0.0f64;
    let mut tv = 0.0;
    for key in keys {
        match (engine.get(key), oracle.get(key)) {
            (Some(p), Some(q)) => {
                maximum = maximum.max((p - q).abs());
                tv += (p - q).abs() / 2.0;
            }
            (Some(p), None) => {
                only_engine.push(key);
                tv += p / 2.0;
            }
            (None, Some(q)) => {
                only_oracle.push(key);
                tv += q / 2.0;
            }
            (None, None) => unreachable!(),
        }
    }
    json!({"matches": only_engine.is_empty() && only_oracle.is_empty() && maximum <= tolerance,
           "engineOutcomes": engine.len(), "oracleOutcomes": oracle.len(),
           "onlyEngine": only_engine, "onlyOracle": only_oracle,
           "maxSharedDiff": maximum, "tv": tv})
}

fn evaluate_position(
    loaded: &LoadedScenario,
    position: &Position,
    options: EnumerateOptions,
) -> Result<OrderedDistribution, String> {
    let decision = scenario_decision(loaded, position).map_err(|e| format!("decision: {e}"))?;
    let mut state = position.state.clone();
    let outcomes = run_decision_mid_turn_with(
        &mut state,
        &position.order,
        &decision,
        &loaded.mid_turn,
        options,
    )
    .map_err(|e| format!("turn: {e}"))?;
    if state != position.state {
        return Err("enumeration did not restore state".into());
    }
    raw_probability_mass(outcomes.iter().map(|outcome| outcome.probability))?;
    let distribution = ordered(engine_distribution(&loaded.meta, &mut state, &outcomes)?);
    if state != position.state {
        return Err("distribution did not restore state".into());
    }
    mass(&distribution)?;
    Ok(distribution)
}

fn gender_contract(
    loaded: &LoadedScenario,
    positions: &[Position],
    distributions: &[OrderedDistribution],
    oracle: &OrderedDistribution,
    tolerance: f64,
) -> Result<Value, String> {
    // These reports are gender-mix.cjs's complete uniform mixture. Restrict this
    // contract to unconditioned starts; setup histories need their own contract.
    if !loaded.setup_turns.is_empty()
        || loaded.start_state.is_some()
        || !loaded.setup_states.is_empty()
    {
        return Err("gender mixture contract requires an unconditioned initial turn".into());
    }
    let undecided: Vec<_> = [SideId::One, SideId::Two]
        .into_iter()
        .flat_map(|side| {
            loaded
                .state
                .side(side)
                .party
                .iter()
                .enumerate()
                .filter(|(_, mon)| !mon.species.is_none() && mon.gender == Gender::Random)
                .map(move |(party, _)| (side, party))
        })
        .collect();
    if undecided.is_empty() {
        return Err("gender contract has no undecided genders".into());
    }
    let expected = 1usize
        .checked_shl(undecided.len() as u32)
        .ok_or("too many genders")?;
    if positions.len() != expected {
        return Err(format!(
            "expected {expected} gender positions, got {}",
            positions.len()
        ));
    }
    let total: f64 = positions.iter().map(|p| p.probability).sum();
    if !total.is_finite() || (total - 1.0).abs() > 1e-12 {
        return Err("gender positions do not have unit mass".into());
    }
    let mut assignments = BTreeSet::new();
    let mut mixed = OrderedDistribution::new();
    let mut branches = Vec::new();
    for (position, distribution) in positions.iter().zip(distributions) {
        if !position.probability.is_finite()
            || (position.probability - 1.0 / expected as f64).abs() > 1e-12
        {
            return Err("gender assignment is not uniformly weighted".into());
        }
        let assignment = undecided
            .iter()
            .map(
                |(side, party)| match position.state.side(*side).party[*party].gender {
                    Gender::Male => Ok('M'),
                    Gender::Female => Ok('F'),
                    _ => Err("an undecided gender did not resolve to male/female"),
                },
            )
            .collect::<Result<String, _>>()?;
        if !assignments.insert(assignment.clone()) {
            return Err("duplicate gender assignment".into());
        }
        let weight = position.probability / total;
        for (state, probability) in distribution {
            *mixed.entry(state.clone()).or_default() += probability * weight;
        }
        branches.push(json!({"gender_assignment": assignment,
                            "setup_probability": position.probability, "normalized_weight": weight,
                            "state_restored": true, "distribution": distribution}));
    }
    mass(&mixed)?;
    let compare = comparison(&mixed, oracle, tolerance);
    Ok(
        json!({"scope": "complete-weighted-gender-mixture", "checks": {
        "expected_assignments": expected, "distinct_assignments": assignments.len(),
        "uniform_weights": true, "matching_probability_mass": total,
        "all_states_restored": true, "oracle_distribution_mass": mass(oracle)?,
        "engine_distribution_mass": mass(&mixed)?},
        "comparison": compare, "engine_distribution": mixed, "branches": branches}),
    )
}

/// Independent Showdown enumerations cover both setup histories. Their canonical
/// before states are identical; their hidden ability orders select different rods.
fn redirect_expected(report: &Value, recipient: usize) -> Result<OrderedDistribution, String> {
    let rows = report["outcomes"].as_array().ok_or("no oracle outcomes")?;
    if rows.len() != 1 || rows[0]["p"].as_f64() != Some(1.0) {
        return Err("redirect contract requires one deterministic oracle outcome".into());
    }
    let mons = rows[0]["state"]["sides"][0]["pokemon"]
        .as_array()
        .ok_or("missing oracle side")?;
    let mut found = [false; 2];
    for mon in mons {
        let Some(slot) = mon["slot"].as_u64().filter(|s| *s < 2) else {
            continue;
        };
        let slot = slot as usize;
        if mon["name"] != ["Rod A", "Rod B"][slot]
            || mon["species"] != "Manectric"
            || mon["ability"] != "lightningrod"
            || mon["boosts"]["spa"].as_i64() != if slot == recipient { Some(1) } else { None }
        {
            return Err("oracle redirect recipient contract changed".into());
        }
        found[slot] = true;
    }
    if found != [true, true] {
        return Err("missing redirect recipients".into());
    }
    checked_oracle_distribution(report)
}

fn redirect_contract(
    loaded: &LoadedScenario,
    positions: &[Position],
    distributions: &[OrderedDistribution],
    report: &Value,
    alternate: &Value,
    tolerance: f64,
) -> Result<Value, String> {
    if report["before"] != alternate["before"]
        || report["mode"] != alternate["mode"]
        || alternate["exact"] != true
        || report["showdownCommit"] != alternate["showdownCommit"]
    {
        return Err("alternate oracle does not share before, mode, and simulator revision".into());
    }
    if positions.len() != 2 {
        return Err(format!(
            "expected two hidden histories, got {}",
            positions.len()
        ));
    }
    let mut recipients = BTreeSet::new();
    let mut branches = Vec::new();
    let mut passed = true;
    for (position, distribution) in positions.iter().zip(distributions) {
        if !position.probability.is_finite() || (position.probability - 0.5).abs() > 1e-12 {
            return Err("hidden histories must have weight 1/2".into());
        }
        let slots = [0, 1].map(|slot| SlotRef {
            side: SideId::One,
            slot,
        });
        let order = slots.map(|slot| position.state.slot(slot).ability_order);
        // ability_order is a compressed rank, not Showdown's unique counter.
        // The complete ordering key is (rank, side, slot); equal ranks retain
        // side/slot order. See State::Slot and Battle::ability_order_key.
        let order_keys = slots.map(|slot| {
            (
                position.state.slot(slot).ability_order,
                slot.side.index(),
                slot.slot,
            )
        });
        // Fail closed if fixture changes invalidate the two-recipient symmetry.
        let canonical =
            canonical_value(&position.state, &loaded.meta).map_err(|e| e.to_string())?;
        for slot in 0..2 {
            let mons = canonical["sides"][0]["pokemon"]
                .as_array()
                .ok_or("missing side")?;
            let mon = mons
                .iter()
                .find(|m| m["slot"] == slot)
                .ok_or("missing redirect slot")?;
            if mon["name"] != ["Rod A", "Rod B"][slot]
                || mon["species"] != "Manectric"
                || mon["ability"] != "lightningrod"
                || mon["boosts"] != json!({})
            {
                return Err("redirect initial-state precondition changed".into());
            }
        }
        if distribution.len() != 1 {
            return Err("a hidden history is no longer deterministic".into());
        }
        let recipient = usize::from(order_keys[1] < order_keys[0]);
        if !recipients.insert(recipient) {
            return Err("both histories select the same recipient".into());
        }
        let expected =
            redirect_expected(if recipient == 1 { report } else { alternate }, recipient)?;
        mass(&expected)?;
        let compare = comparison(distribution, &expected, tolerance);
        passed &= compare["matches"] == true;
        branches.push(json!({"ability_order": order, "ability_order_keys": order_keys, "recipient_slot": recipient,
            "setup_probability": position.probability, "normalized_weight": 0.5,
            "validation": "direct-oracle", "oracle_history": if recipient == 1 { "original-seed" } else { "alternate-seed" },
            "state_restored": true, "distribution": distribution, "comparison": compare}));
    }
    if distributions[0] == distributions[1] {
        return Err("hidden histories did not change redirection".into());
    }
    Ok(
        json!({"scope": "both-hidden-histories-with-independent-oracles",
        "checks": {"expected_histories": 2, "distinct_recipients": recipients.len(),
          "uniform_weights": true, "matching_probability_mass": 1.0,
          "direct_oracle_histories": 2, "metamorphic_histories": 0, "all_states_restored": true},
        "comparison": {"matches": passed}, "branches": branches}),
    )
}

fn check(
    scenario: &Path,
    report: &Value,
    contract: &str,
    tolerance: f64,
    alternate: Option<&Value>,
) -> Result<Value, String> {
    if !tolerance.is_finite() || tolerance < 0.0 {
        return Err("invalid tolerance".into());
    }
    let loaded = load_scenario_file(scenario).map_err(|e| e.to_string())?;
    let positions = scenario_positions(&loaded).map_err(|e| format!("setup: {e}"))?;
    let generated = positions.len();
    let generated_mass = validate_contract_parents(&loaded, &positions, &report["before"])?;
    let matching = positions;
    let rolls = match report["mode"].as_str() {
        Some("full") => RollMode::Full,
        Some("extremes") => RollMode::Extremes,
        Some("fixed") => report["roll"]
            .as_u64()
            .and_then(|r| u8::try_from(r).ok())
            .and_then(RollMode::fixed)
            .ok_or("invalid fixed roll")?,
        _ => return Err("invalid report mode".into()),
    };
    let distributions = matching
        .iter()
        .map(|p| evaluate_position(&loaded, p, EnumerateOptions { rolls }))
        .collect::<Result<Vec<_>, _>>()?;
    let oracle = checked_oracle_distribution(report)?;
    mass(&oracle)?;
    let mut result = match contract {
        GENDERS => gender_contract(&loaded, &matching, &distributions, &oracle, tolerance)?,
        REDIRECT => redirect_contract(
            &loaded,
            &matching,
            &distributions,
            report,
            alternate.ok_or("alternate oracle report is required")?,
            tolerance,
        )?,
        _ => return Err(format!("unknown oracle contract {contract:?}")),
    };
    result["schema_version"] = json!(1);
    result["contract"] = json!(contract);
    result["status"] = json!(if result["comparison"]["matches"] == true {
        "match"
    } else {
        "mismatch"
    });
    result["generated_positions"] = json!(generated);
    result["generated_probability_mass"] = json!(generated_mass);
    result["matching_positions"] = json!(matching.len());
    result["mode"] = report["mode"].clone();
    result["tolerance"] = json!(tolerance);
    Ok(result)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = (|| {
        if !matches!(args.len(), 6 | 8)
            || args[2] != "--contract"
            || args[4] != "--tolerance"
            || (args.len() == 8 && args[6] != "--alternate-report")
        {
            return Err(
                "expected SCENARIO REPORT --contract CONTRACT --tolerance NUMBER".to_owned(),
            );
        }
        let report: Value =
            serde_json::from_slice(&std::fs::read(&args[1]).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        let alternate: Option<Value> = if args.len() == 8 {
            Some(
                serde_json::from_slice(&std::fs::read(&args[7]).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?,
            )
        } else {
            None
        };
        check(
            Path::new(&args[0]),
            &report,
            &args[3],
            args[5].parse().map_err(|_| "invalid tolerance")?,
            alternate.as_ref(),
        )
    })();
    let output = result.unwrap_or_else(|error| {
        json!({"schema_version": 1,
        "contract": args.get(3), "status": "engine-error", "error": error})
    });
    println!("{output}");
    if output["status"] == "match" {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(name: &str, contract: &str) -> Value {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../oracle");
        let report: Value = serde_json::from_slice(
            &std::fs::read(root.join(format!("expected/{name}.turn.json"))).unwrap(),
        )
        .unwrap();
        let alternate = if contract == REDIRECT {
            Some(serde_json::from_slice(&std::fs::read(root.join("../benchmarks/agreement/data/contracts/ss-redirect-tie-hidden-order-rod-a.turn.json")).unwrap()).unwrap())
        } else {
            None
        };
        check(
            &root.join(format!("scenarios/{name}.json")),
            &report,
            contract,
            1e-9,
            alternate.as_ref(),
        )
        .unwrap()
    }
    #[test]
    fn three_gender_fixtures_compare_the_complete_mixture() {
        for name in [
            "rr-attract-undecided-gender",
            "rr-cute-charm-undecided-gender",
            "rr-rivalry-undecided-gender",
        ] {
            let result = fixture(name, GENDERS);
            assert_eq!(result["status"], "match", "{name}: {result}");
            assert_eq!(result["matching_positions"], 16);
            assert_eq!(result["checks"]["distinct_assignments"], 16);
        }
    }
    #[test]
    fn redirect_checks_both_histories_with_explicit_oracle_coverage() {
        let result = fixture("ss-redirect-tie-hidden-order", REDIRECT);
        assert_eq!(result["status"], "match", "{result}");
        assert_eq!(result["checks"]["direct_oracle_histories"], 2);
        assert_eq!(result["checks"]["metamorphic_histories"], 0);
        let branches = result["branches"].as_array().unwrap();
        // One fixture history has equal compressed ranks but an unambiguous
        // (rank, side, slot) order: Rod A started before Rod B.
        let tied = branches
            .iter()
            .find(|b| b["ability_order"][0] == b["ability_order"][1])
            .unwrap();
        assert_eq!(tied["recipient_slot"], 0);
        assert_ne!(tied["ability_order_keys"][0], tied["ability_order_keys"][1]);
    }
    #[test]
    fn missing_duplicate_and_wrongly_weighted_gender_branches_are_rejected() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../oracle");
        let loaded =
            load_scenario_file(root.join("scenarios/rr-attract-undecided-gender.json")).unwrap();
        let positions = scenario_positions(&loaded).unwrap();
        let oracle: OrderedDistribution = [("state".into(), 1.0)].into_iter().collect();
        let distributions = vec![oracle.clone(); positions.len()];
        assert!(gender_contract(
            &loaded,
            &positions[..15],
            &distributions[..15],
            &oracle,
            1e-9
        )
        .is_err());
        let mut duplicated = positions.clone();
        duplicated[1] = duplicated[0].clone();
        assert!(gender_contract(&loaded, &duplicated, &distributions, &oracle, 1e-9).is_err());
        let mut wrong_weights = positions;
        wrong_weights[0].probability += 0.01;
        wrong_weights[1].probability -= 0.01;
        assert!(gender_contract(&loaded, &wrong_weights, &distributions, &oracle, 1e-9).is_err());
    }
    #[test]
    fn independent_oracle_state_change_is_not_hidden_by_contract_success() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../oracle");
        let report: Value = serde_json::from_slice(
            &std::fs::read(root.join("expected/ss-redirect-tie-hidden-order.turn.json")).unwrap(),
        )
        .unwrap();
        let mut alternate: Value = serde_json::from_slice(&std::fs::read(root.join("../benchmarks/agreement/data/contracts/ss-redirect-tie-hidden-order-rod-a.turn.json")).unwrap()).unwrap();
        alternate["outcomes"][0]["state"]["turn"] = json!(99);
        let result = check(
            &root.join("scenarios/ss-redirect-tie-hidden-order.json"),
            &report,
            REDIRECT,
            1e-9,
            Some(&alternate),
        )
        .unwrap();
        assert_eq!(result["status"], "mismatch");
    }
    #[test]
    fn raw_negative_weights_cannot_hide_behind_canonical_merging() {
        let report = json!({"outcomes": [
            {"p": 1.2, "state": {"turn": 2}},
            {"p": -0.2, "state": {"turn": 2}}
        ]});
        // This demonstrates why checking only the already-merged map was insufficient.
        assert!(mass(&ordered(report_distribution(&report).unwrap())).is_ok());
        assert!(checked_oracle_distribution(&report).is_err());
        let outcomes = [1.2, -0.2].map(|probability| lab_engine::instruction::Outcome {
            probability,
            instructions: Vec::new(),
            suspension: None,
        });
        assert!(raw_probability_mass(outcomes.iter().map(|o| o.probability)).is_err());
        for invalid in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
            assert!(raw_probability_mass([invalid]).is_err());
        }
    }
    #[test]
    fn extra_nonmatching_parent_cannot_be_discarded_before_mass_checks() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../oracle");
        let loaded =
            load_scenario_file(root.join("scenarios/rr-attract-undecided-gender.json")).unwrap();
        let mut positions = scenario_positions(&loaded).unwrap();
        let before = canonical_value(&positions[0].state, &loaded.meta).unwrap();
        assert!(validate_contract_parents(&loaded, &positions, &before).is_ok());
        let mut additional = positions[0].clone();
        additional.state.turn += 1;
        positions.push(additional);
        // Keep the whole generated distribution valid so the canonical-scope
        // guard, rather than the mass guard, must catch the extra parent.
        let weight = 1.0 / positions.len() as f64;
        for position in &mut positions {
            position.probability = weight;
        }
        assert!(raw_probability_mass(positions.iter().map(|p| p.probability)).is_ok());
        assert!(validate_contract_parents(&loaded, &positions, &before).is_err());
    }
    #[test]
    fn changing_an_oracle_probability_cannot_pass() {
        let a: OrderedDistribution = [("a".into(), 0.25), ("b".into(), 0.75)]
            .into_iter()
            .collect();
        let b: OrderedDistribution = [("a".into(), 0.5), ("b".into(), 0.5)].into_iter().collect();
        assert_eq!(comparison(&a, &b, 1e-9)["matches"], false);
        assert!(mass(&[("a".into(), f64::NAN)].into_iter().collect()).is_err());
    }
}
