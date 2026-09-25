//! Runs every doubles library team (`teams/library/doubles/m-c/*/team.json`) in VGC (team
//! preview keeps 4) against one fixed opponent and reports what the turn engine refuses,
//! grouped by the `TurnError::Unsupported` reason (work plan O106).
//!
//! Usage: lab-library --out <file.md> [--teams <dir>]
//!
//! Every team is checked in three team previews (`1234`, `3456`, `5612`), so that every member
//! leads once. For each preview:
//! 1. the battle start (`turn::enumerate_start`: the leads' switch-in effects);
//! 2. from the start's first outcome, the "protect turn": each lead uses Protect (or, without
//!    it, its first move);
//! 3. each move of each lead, the partner protecting as above;
//! 4. Mega Evolution of each eligible lead (with its protect-turn move).
//!
//! The opponent (inert abilities, no items) protects every turn. A refusal is recorded with
//! the engine's exact `TurnError::Unsupported` text; the report groups them by that text with
//! the leading `<Pokémon>: ` removed. Choices the engine rejects as illegal (a status move
//! under Assault Vest, a move without PP) are counted apart: they are not support gaps.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use lab_engine::action::{JointAction, SlotAction};
use lab_engine::dex::{moves, SpeciesId};
use lab_engine::gimmick::Gimmick;
use lab_engine::rules::Ruleset;
use lab_engine::state::{SideId, SlotRef};
use lab_engine::turn::{
    enumerate_start, enumerate_turn, takes_target, valid_target_loc, TurnError,
};
use lab_engine::Doubles;
use lab_scenario::{load_scenario_str, VGC_FORMAT};

const PREVIEWS: [&str; 3] = ["1234", "3456", "5612"];

/// The fixed opponent: inert abilities, no items, Protect first.
fn opponent() -> Value {
    let mon = |species: &str, nature: &str| {
        json!({"species": species, "item": "", "ability": "Honey Gather", "nature": nature,
               "evs": {"hp": 32, "def": 2, "spd": 32}, "moves": ["Protect", "Tackle"],
               "level": 50})
    };
    json!([
        mon("Snorlax", "Careful"),
        mon("Kangaskhan", "Careful"),
        mon("Hariyama", "Careful"),
        mon("Machamp", "Careful")
    ])
}

/// What one check gave.
enum Verdict {
    Ok,
    Refused(String),
    /// The engine rejected the choice itself (not a support gap).
    Illegal(String),
}

fn verdict<T>(result: Result<T, TurnError>) -> Verdict {
    match result {
        Ok(_) => Verdict::Ok,
        Err(TurnError::Unsupported(why)) => Verdict::Refused(why),
        Err(other) => Verdict::Illegal(other.to_string()),
    }
}

/// A lead's move choice: foe 1 for a move that takes a target and may aim at a foe, else the
/// ally, else no target.
fn move_action(state: &Doubles, slot: SlotRef, index: u8, gimmick: Gimmick) -> SlotAction {
    let mon = state.active(slot).expect("a lead");
    let target_kind = mon.moves[index as usize].id.data().target;
    let target = if !takes_target(2, target_kind) {
        0
    } else if valid_target_loc(2, slot, 1, target_kind) {
        1
    } else {
        // The ally's position, as the user sees it.
        -((1 - slot.slot as i8) + 1)
    };
    SlotAction::Move {
        index,
        target,
        gimmick,
    }
}

/// The lead's protect-turn move: Protect if it knows it, else its first move.
fn protect_index(state: &Doubles, slot: SlotRef) -> u8 {
    let mon = state.active(slot).expect("a lead");
    mon.moves
        .iter()
        .position(|m| m.id == moves::PROTECT)
        .unwrap_or(0) as u8
}

fn opponent_action() -> JointAction<2> {
    let protect = SlotAction::Move {
        index: 0,
        target: 0,
        gimmick: Gimmick::None,
    };
    [protect, protect]
}

struct TeamReport {
    id: String,
    /// Start + protect turn of every preview went through.
    first_turns_ok: bool,
    /// Previews whose start and protect turn went through.
    previews_ok: usize,
    /// Every check went through.
    all_ok: bool,
    /// (check, exact reason) of every refusal.
    refusals: Vec<(String, String)>,
    /// (check, error) of every choice the engine rejected as illegal.
    illegal: Vec<(String, String)>,
    /// The library index's status (`validated`, `source-complete-sp-unknown`, `needs-review`).
    status: String,
    load_error: Option<String>,
}

fn check_team(id: &str, team: &Value, base: &Path) -> TeamReport {
    let mut report = TeamReport {
        id: id.to_owned(),
        first_turns_ok: true,
        previews_ok: 0,
        all_ok: true,
        refusals: Vec::new(),
        illegal: Vec::new(),
        status: String::new(),
        load_error: None,
    };
    for preview in PREVIEWS {
        let scenario = json!({
            "format": VGC_FORMAT,
            "p1": {"team": team, "order": preview},
            "p2": {"team": opponent(), "order": "1234"},
        });
        let loaded = match load_scenario_str(&scenario.to_string(), base) {
            Ok(loaded) => loaded,
            Err(error) => {
                report.load_error = Some(error.to_string());
                report.first_turns_ok = false;
                report.all_ok = false;
                return report;
            }
        };
        let mut state = loaded.state.clone();
        let starts = match enumerate_start(&mut state) {
            Ok(starts) => starts,
            Err(error) => {
                report.first_turns_ok = false;
                report.all_ok = false;
                match verdict::<()>(Err(error)) {
                    Verdict::Refused(why) => report.refusals.push((format!("{preview} 시작"), why)),
                    Verdict::Illegal(why) => report.illegal.push((format!("{preview} 시작"), why)),
                    Verdict::Ok => {}
                }
                continue;
            }
        };
        state.apply(&starts[0].instructions);

        let leads = [0u8, 1].map(|slot| SlotRef {
            side: SideId::One,
            slot,
        });
        let base_action = |state: &Doubles, slot: SlotRef| {
            move_action(state, slot, protect_index(state, slot), Gimmick::None)
        };
        let mut checks: Vec<(String, bool, JointAction<2>)> = Vec::new();
        let protect_turn = [base_action(&state, leads[0]), base_action(&state, leads[1])];
        checks.push((format!("{preview} 방어 턴"), true, protect_turn));
        for (i, &slot) in leads.iter().enumerate() {
            let mon = state.active(slot).expect("a lead");
            for (index, m) in mon.moves.iter().enumerate() {
                if m.id.is_none() {
                    continue;
                }
                let mut action = protect_turn;
                action[i] = move_action(&state, slot, index as u8, Gimmick::None);
                checks.push((format!("{preview} {}", m.id.data().name), false, action));
            }
            if mon.gimmicks.contains(Gimmick::Mega) {
                let mut action = protect_turn;
                action[i] = move_action(&state, slot, protect_index(&state, slot), Gimmick::Mega);
                checks.push((format!("{preview} 메가진화"), false, action));
            }
        }
        let mut preview_ok = true;
        for (name, is_first_turn, action) in checks {
            let result = enumerate_turn(
                &mut state,
                Ruleset::CHAMPIONS_MC,
                [action, opponent_action()],
            );
            match verdict(result) {
                Verdict::Ok => {}
                Verdict::Refused(why) => {
                    report.all_ok = false;
                    if is_first_turn {
                        report.first_turns_ok = false;
                        preview_ok = false;
                    }
                    report.refusals.push((name, why));
                }
                Verdict::Illegal(why) => report.illegal.push((name, why)),
            }
        }
        if preview_ok {
            report.previews_ok += 1;
        }
    }
    report
}

/// The reason without its leading `<Pokémon>: `, and that Pokémon.
fn split_reason(reason: &str) -> (Option<&str>, &str) {
    match reason.split_once(": ") {
        Some((head, tail)) if SpeciesId::from_name(head).is_some() => (Some(head), tail),
        _ => (None, reason),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut out = None;
    let mut dir = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out" => {
                i += 1;
                out = args.get(i).cloned();
            }
            "--teams" => {
                i += 1;
                dir = args.get(i).map(PathBuf::from);
            }
            other => panic!("unexpected argument {other}"),
        }
        i += 1;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = dir.unwrap_or_else(|| root.join("teams/library/doubles/m-c"));

    let mut teams: Vec<(String, PathBuf)> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("team.json").is_file())
        .map(|p| {
            let id = p.file_name().unwrap().to_string_lossy().into_owned();
            (id, p.join("team.json"))
        })
        .collect();
    teams.sort();

    // `teams/library/index.json` statuses, when the library has one.
    let index: Value = std::fs::read_to_string(dir.join("../../index.json"))
        .ok()
        .and_then(|text| serde_json::from_str(text.trim_start_matches('\u{feff}')).ok())
        .unwrap_or(Value::Null);
    let status_of = |id: &str| -> String {
        index["teams"]
            .as_array()
            .and_then(|teams| teams.iter().find(|t| t["id"] == id))
            .and_then(|t| t["status"].as_str())
            .unwrap_or("?")
            .to_owned()
    };

    let mut reports = Vec::new();
    for (id, path) in &teams {
        let text = std::fs::read_to_string(path).expect("read team.json");
        let team: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let mut report = check_team(id, &team, &dir);
        report.status = status_of(id);
        reports.push(report);
    }

    let total = reports.len();
    let loaded = reports.iter().filter(|r| r.load_error.is_none()).count();
    let first_ok = reports.iter().filter(|r| r.first_turns_ok).count();
    let all_ok = reports.iter().filter(|r| r.all_ok).count();
    let previews = total * PREVIEWS.len();
    let previews_ok: usize = reports.iter().map(|r| r.previews_ok).sum();

    // reason (without the Pokémon) → team → Pokémon.
    let mut groups: BTreeMap<String, BTreeMap<String, Vec<String>>> = BTreeMap::new();
    for r in &reports {
        for (_, reason) in &r.refusals {
            let (who, what) = split_reason(reason);
            let mons = groups
                .entry(what.to_owned())
                .or_default()
                .entry(r.id.clone())
                .or_default();
            if let Some(who) = who {
                if !mons.iter().any(|m| m == who) {
                    mons.push(who.to_owned());
                }
            }
        }
    }
    let mut groups: Vec<_> = groups.into_iter().collect();
    groups.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));

    let mut md = String::new();
    writeln!(md, "# 라이브러리 더블 팀 지원 현황 (자동 생성)\n").unwrap();
    writeln!(
        md,
        "`cargo run -p lab-scenario --release --bin lab-library -- --out <이 파일>`로 만든다. \
         `teams/library/doubles/m-c/*/team.json` {total}팀을 VGC 형식(`{VGC_FORMAT}`, 팀 \
         프리뷰에서 4마리 선출)으로 고정 상대(무효 특성·도구 없음, 매 턴 방어)와 붙인다. \
         팀마다 선출 `1234`·`3456`·`5612`(모든 멤버가 한 번씩 선두)에서 (1) 배틀 시작 \
         등장 효과, (2) 선두 둘이 방어(없으면 첫 기술)하는 첫 턴, (3) 선두 각각의 기술 4개, \
         (4) 메가진화 가능한 선두의 메가진화를 엔진으로 실행한다. 이유는 엔진이 낸 \
         `TurnError::Unsupported` 문자열 그대로이고, 묶을 때 앞의 `<포켓몬>: `만 뗐다. \
         벤치 멤버의 등장·교체, 선택하지 않은 기술 조합, 둘째 턴 이후는 검사하지 않는다. \
         직접 편집하지 않는다.\n"
    )
    .unwrap();
    writeln!(md, "## 요약\n").unwrap();
    writeln!(md, "- 팀 {total}개, 로더 통과 {loaded}개.").unwrap();
    writeln!(
        md,
        "- 세 선출 모두 시작 + 방어 턴이 실행되는 팀: {first_ok}개 (거부 {}개).",
        total - first_ok
    )
    .unwrap();
    writeln!(
        md,
        "- 선출 {previews}개(팀 × 3) 중 시작 + 방어 턴이 실행되는 선출: {previews_ok}개."
    )
    .unwrap();
    writeln!(md, "- 모든 검사(기술·메가진화 포함) 통과: {all_ok}개.\n").unwrap();

    writeln!(md, "## 거부 이유 (팀 수순)\n").unwrap();
    writeln!(md, "| 이유 | 팀 수 | 팀 (포켓몬) |").unwrap();
    writeln!(md, "|---|---:|---|").unwrap();
    for (reason, teams) in &groups {
        let list: Vec<String> = teams
            .iter()
            .map(|(team, mons)| {
                if mons.is_empty() {
                    team.clone()
                } else {
                    format!("{team} ({})", mons.join(", "))
                }
            })
            .collect();
        let reason = reason.replace('|', "\\|");
        writeln!(md, "| {reason} | {} | {} |", teams.len(), list.join(", ")).unwrap();
    }
    writeln!(md).unwrap();

    writeln!(md, "## 팀별\n").unwrap();
    writeln!(
        md,
        "| 팀 | 라이브러리 상태 | 시작+방어 턴 | 전체 | 거부 검사 수 | 불법 선택 | 첫 거부 |"
    )
    .unwrap();
    writeln!(md, "|---|---|---|---|---:|---:|---|").unwrap();
    for r in &reports {
        let yes_no = |b: bool| if b { "통과" } else { "거부" };
        let first = match (&r.load_error, r.refusals.first()) {
            (Some(error), _) => format!("로더: {error}"),
            (None, Some((check, reason))) => format!("{check}: {reason}"),
            (None, None) => String::new(),
        };
        writeln!(
            md,
            "| {} | {} | {} | {} | {} | {} | {} |",
            r.id,
            r.status,
            yes_no(r.first_turns_ok),
            yes_no(r.all_ok),
            r.refusals.len(),
            r.illegal.len(),
            first.replace('|', "\\|")
        )
        .unwrap();
    }

    let illegal: Vec<String> = reports
        .iter()
        .flat_map(|r| {
            r.illegal
                .iter()
                .map(move |(check, why)| format!("- {} {check}: {why}", r.id))
        })
        .collect();
    if !illegal.is_empty() {
        writeln!(
            md,
            "\n## 엔진이 불법 선택으로 거절한 검사 (지원 공백 아님)\n\n{}",
            illegal.join("\n")
        )
        .unwrap();
    }

    match out {
        Some(path) => std::fs::write(&path, md).expect("write report"),
        None => print!("{md}"),
    }
    eprintln!(
        "lab-library: {total} teams, {loaded} loaded, {first_ok} with every start and protect \
         turn supported ({previews_ok}/{previews} previews), {all_ok} with every check supported"
    );
}
