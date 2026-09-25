//! Writes the turn engine's coverage of the Champions dex as Markdown: every move, ability
//! and item classified by the support gate, weighted by how often the team library uses it.
//!
//! Usage: lab-coverage [--out <file.md>] [--teams <dir>]...
//!
//! `--teams` directories are scanned recursively for `team.json` files (Showdown set JSON
//! arrays). Default: `teams/library/doubles/m-c` and the lab's own `teams/*.json`.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use lab_engine::dex::{to_id, AbilityId, ItemId, MoveId};
use lab_engine::turn::coverage::{ability_support, item_support, move_support, Support};
use lab_scenario::parse_team;

#[derive(Default)]
struct Usage {
    moves: BTreeMap<String, usize>,
    abilities: BTreeMap<String, usize>,
    items: BTreeMap<String, usize>,
    teams: usize,
    sets: usize,
}

fn scan(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan(&path, files);
        } else if path.extension().is_some_and(|e| e == "json") {
            files.push(path);
        }
    }
}

fn count(path: &Path, usage: &mut Usage) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let Ok(team) = parse_team(&text) else {
        return;
    };
    if team.is_empty() {
        return;
    }
    usage.teams += 1;
    for set in &team {
        usage.sets += 1;
        for m in &set.moves {
            *usage.moves.entry(to_id(m)).or_default() += 1;
        }
        if let Some(a) = &set.ability {
            *usage.abilities.entry(to_id(a)).or_default() += 1;
        }
        if let Some(i) = &set.item {
            if !i.is_empty() {
                *usage.items.entry(to_id(i)).or_default() += 1;
            }
        }
    }
}

struct Row {
    name: &'static str,
    id: &'static str,
    uses: usize,
    support: Support,
}

fn section(out: &mut String, title: &str, rows: &[Row]) {
    let supported = rows.iter().filter(|r| r.support.is_supported()).count();
    let no_switch = rows
        .iter()
        .filter(|r| matches!(r.support, Support::NoSwitchIn { .. }))
        .count();
    let used: Vec<&Row> = rows.iter().filter(|r| r.uses > 0).collect();
    let used_ok = used.iter().filter(|r| r.support.is_supported()).count();
    writeln!(out, "## {title}\n").unwrap();
    writeln!(
        out,
        "- 전체 {}개 중 지원 {}개, 등장 효과만 미지원 {}개, 미지원 {}개.",
        rows.len(),
        supported,
        no_switch,
        rows.len() - supported - no_switch
    )
    .unwrap();
    writeln!(
        out,
        "- 라이브러리 사용 {}개 중 지원 {}개 ({:.0}%).\n",
        used.len(),
        used_ok,
        if used.is_empty() {
            100.0
        } else {
            100.0 * used_ok as f64 / used.len() as f64
        }
    )
    .unwrap();

    writeln!(out, "### 라이브러리에서 쓰이는데 미지원 (사용 횟수순)\n").unwrap();
    writeln!(out, "| 이름 | 사용 | 상태 | 이유 |").unwrap();
    writeln!(out, "|---|---:|---|---|").unwrap();
    let mut missing: Vec<&Row> = used
        .iter()
        .copied()
        .filter(|r| !r.support.is_supported())
        .collect();
    missing.sort_by(|a, b| b.uses.cmp(&a.uses).then(a.name.cmp(b.name)));
    for r in &missing {
        let (status, reason) = describe(&r.support);
        writeln!(out, "| {} | {} | {status} | {reason} |", r.name, r.uses).unwrap();
    }
    writeln!(out).unwrap();

    writeln!(out, "### 라이브러리에서 쓰이고 지원됨\n").unwrap();
    let mut ok: Vec<&Row> = used
        .iter()
        .copied()
        .filter(|r| r.support.is_supported())
        .collect();
    ok.sort_by(|a, b| b.uses.cmp(&a.uses).then(a.name.cmp(b.name)));
    let names: Vec<String> = ok
        .iter()
        .map(|r| format!("{} ({})", r.name, r.uses))
        .collect();
    writeln!(out, "{}\n", names.join(", ")).unwrap();

    writeln!(out, "### 나머지 미지원 (이유별)\n").unwrap();
    let mut by_reason: BTreeMap<String, Vec<&Row>> = BTreeMap::new();
    for r in rows
        .iter()
        .filter(|r| r.uses == 0 && !r.support.is_supported())
    {
        let (status, reason) = describe(&r.support);
        by_reason
            .entry(format!("{status}: {reason}"))
            .or_default()
            .push(r);
    }
    let mut groups: Vec<_> = by_reason.into_iter().collect();
    groups.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));
    for (reason, rows) in groups {
        let names: Vec<&str> = rows.iter().map(|r| r.name).collect();
        writeln!(out, "- **{reason}** ({}): {}", rows.len(), names.join(", ")).unwrap();
    }
    writeln!(out).unwrap();
}

/// A reason string with the move name stripped (the table has its own name column).
fn describe(support: &Support) -> (&'static str, String) {
    match support {
        Support::Supported => ("지원", String::new()),
        Support::NoSwitchIn { handlers } => ("등장 효과 미구현", format!("{handlers:?}")),
        Support::Unsupported { reason } => {
            let reason = match reason.split_once(": ") {
                Some((head, tail)) if head.starts_with("move ") => tail.to_owned(),
                _ => reason.clone(),
            };
            ("미지원", reason)
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut out = None;
    let mut dirs = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out" => {
                i += 1;
                out = args.get(i).cloned();
            }
            "--teams" => {
                i += 1;
                dirs.push(PathBuf::from(&args[i]));
            }
            other => panic!("unexpected argument {other}"),
        }
        i += 1;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    if dirs.is_empty() {
        dirs.push(root.join("teams/library/doubles/m-c"));
        dirs.push(root.join("teams"));
    }

    let mut usage = Usage::default();
    let mut files = Vec::new();
    for dir in &dirs {
        if dir.is_file() {
            files.push(dir.clone());
        } else {
            scan(dir, &mut files);
        }
    }
    files.sort();
    files.dedup();
    for f in &files {
        count(f, &mut usage);
    }

    let moves: Vec<Row> = MoveId::all()
        .map(|id| Row {
            name: id.data().name,
            id: id.id(),
            uses: usage.moves.get(id.id()).copied().unwrap_or(0),
            support: move_support(id),
        })
        .collect();
    let abilities: Vec<Row> = AbilityId::all()
        .map(|id| Row {
            name: id.data().name,
            id: id.id(),
            uses: usage.abilities.get(id.id()).copied().unwrap_or(0),
            support: ability_support(id),
        })
        .collect();
    let items: Vec<Row> = ItemId::all()
        .map(|id| Row {
            name: id.data().name,
            id: id.id(),
            uses: usage.items.get(id.id()).copied().unwrap_or(0),
            support: item_support(id),
        })
        .collect();
    let _ = moves.first().map(|r| r.id);

    let mut md = String::new();
    writeln!(md, "# 턴 엔진 데이터 커버리지 (자동 생성)\n").unwrap();
    writeln!(
        md,
        "`cargo run -p lab-scenario --release --bin lab-coverage -- --out engine/COVERAGE.md`로 만든다. \
         `lab-coverage`가 `engine/core/src/turn/support.rs`의 지원 검사를 dex 전체에 적용해 만든다. \
         사용 횟수는 스캔한 팀 파일 {}개({} 세트)에서 센다. 직접 편집하지 않는다.\n",
        usage.teams, usage.sets
    )
    .unwrap();
    section(&mut md, "기술", &moves);
    section(&mut md, "특성", &abilities);
    section(&mut md, "도구", &items);

    match out {
        Some(path) => std::fs::write(&path, md).expect("write report"),
        None => print!("{md}"),
    }
}
