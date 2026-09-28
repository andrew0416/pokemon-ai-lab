//! Board P2a/P2b: `legal_joint_actions` (each slot's candidates checked once, duplicates
//! removed per slot) gives the same joint actions in the same order as the reference (`check_side`
//! over every joint action, quadratic de-duplication), for both sides of every oracle scenario
//! position.

use std::path::{Path, PathBuf};

use lab_engine::rules::Ruleset;
use lab_engine::state::SideId;
use lab_engine::turn::{legal_joint_actions, legal_joint_actions_reference};
use lab_scenario::{load_scenario_file, scenario_positions};

fn scenario_files() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../oracle/scenarios");
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            name.ends_with(".json") && name.matches('.').count() == 1
        })
        .collect();
    files.sort();
    files
}

#[test]
fn legal_joint_actions_match_the_reference() {
    let ruleset = Ruleset::CHAMPIONS_MC;
    let mut positions_checked = 0usize;
    let mut actions = 0usize;
    for path in scenario_files() {
        let Ok(loaded) = load_scenario_file(&path) else {
            continue;
        };
        let Ok(positions) = scenario_positions(&loaded) else {
            continue;
        };
        for position in &positions {
            positions_checked += 1;
            for side in [SideId::One, SideId::Two] {
                let fast = legal_joint_actions(&position.state, ruleset, side);
                let reference = legal_joint_actions_reference(&position.state, ruleset, side);
                assert_eq!(fast, reference, "{} {side:?}", path.display());
                actions += fast.len();
            }
        }
    }
    eprintln!("{positions_checked} positions, {actions} joint actions");
    assert!(positions_checked > 1000);
}
