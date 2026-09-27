"""GitHub Actions 탐색 잡(.github/workflows/search.yml)의 입력을 꾸리고 결과를 되가져온다.

    python engine/scripts/actions_job.py pack   runs/plan-20260926 plan-20260926
    python engine/scripts/actions_job.py merge  <artifacts-dir> <merged-dir>        # 워크플로의 collect 잡이 씀
    python engine/scripts/actions_job.py unpack <내려받은 zip 또는 디렉터리> runs/actions-plan-20260926

pack: 실행 디렉터리의 *-vs-*.json 시나리오를 engine/jobs/<name>/ 로 복사한다(runs/ 는 gitignore라
러너에 없다). 팀 경로는 시나리오 파일 기준 상대 경로이므로 새 위치에서 같은 팀 파일을 가리키도록
고쳐 쓴다. 팀 파일 자체는 복사하지도 고치지도 않는다(teams/ 원본은 이미 추적됨). 원본 시나리오의
`description`과 원래 경로를 `provenance.json`에 남긴다.

merge: 러너별 아티팩트(plan-<scenario>/…)의 summary.*.json 을 하나로 합치고 out/·leads/·provenance 를
한 디렉터리에 모은다. 같은 시나리오·위치의 항목이 겹치면 뒤의 것을 버리지 않고 둘 다 두며 경고한다.

unpack: 내려받은 summary-* 아티팩트(zip 또는 풀린 디렉터리)를 runs/actions-<name>/ 로 옮기고
README.md 에 provenance(커밋·러너·인자)를 표로 적는다. 기존 runs/ 디렉터리는 덮어쓰지 않는다.
"""
import json
import os
import pathlib
import shutil
import sys
import zipfile

ROOT = pathlib.Path(__file__).resolve().parents[2]


def rel_from(new_dir: pathlib.Path, old_dir: pathlib.Path, path: str) -> str:
    target = (old_dir / path).resolve()
    return os.path.relpath(target, new_dir.resolve()).replace(os.sep, "/")


def pack(run_dir: str, name: str) -> None:
    src = pathlib.Path(run_dir)
    dst = ROOT / "engine" / "jobs" / name
    if dst.exists():
        sys.exit(f"{dst} exists; choose another name (jobs are not overwritten)")
    dst.mkdir(parents=True)
    prov = {"source_run_dir": str(src), "scenarios": []}
    for sc in sorted(src.glob("*-vs-*.json")):
        d = json.load(open(sc, encoding="utf-8"))
        for side in ("p1", "p2"):
            if side in d and "team" in d[side]:
                d[side]["team"] = rel_from(dst, sc.parent, d[side]["team"])
        for key in ("believedTeam", "believed_team"):
            if key in d:
                d[key] = rel_from(dst, sc.parent, d[key])
        json.dump(d, open(dst / sc.name, "w", encoding="utf-8"), indent=1, ensure_ascii=False)
        prov["scenarios"].append({"file": sc.name, "from": str(sc), "description": d.get("description", "")})
    json.dump(prov, open(dst / "provenance.json", "w", encoding="utf-8"), indent=1, ensure_ascii=False)
    print(f"packed {len(prov['scenarios'])} scenarios into {dst}")


def merge(artifacts: str, out: str) -> None:
    a = pathlib.Path(artifacts)
    o = pathlib.Path(out)
    (o / "out").mkdir(parents=True, exist_ok=True)
    summaries: dict[str, list] = {}
    provenance = []
    for shard in sorted(a.glob("plan-*")):
        for f in shard.rglob("*"):
            if f.is_dir():
                continue
            rel = f.relative_to(shard)
            if rel.name.startswith("summary") and rel.suffix == ".json":
                summaries.setdefault(rel.name, []).extend(json.load(open(f, encoding="utf-8")))
            elif rel.name.startswith("provenance."):
                provenance.append(json.load(open(f, encoding="utf-8")))
            else:
                dest = o / rel
                dest.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(f, dest)
    for name, entries in summaries.items():
        keys = [(e.get("scenario"), e.get("position")) for e in entries]
        if len(set(keys)) != len(keys):
            print(f"warning: duplicate scenario/position entries in {name}", file=sys.stderr)
        json.dump(entries, open(o / name, "w", encoding="utf-8"), indent=1, ensure_ascii=False)
    json.dump(provenance, open(o / "provenance.json", "w", encoding="utf-8"), indent=1)
    print(f"merged {len(provenance)} shards, summaries: {', '.join(summaries) or 'none'}")


def unpack(downloaded: str, run_dir: str) -> None:
    dst = pathlib.Path(run_dir)
    if dst.exists():
        sys.exit(f"{dst} exists; runs are not overwritten")
    src = pathlib.Path(downloaded)
    if src.suffix == ".zip":
        dst.mkdir(parents=True)
        with zipfile.ZipFile(src) as z:
            z.extractall(dst)
    else:
        shutil.copytree(src, dst)
    prov_path = dst / "provenance.json"
    prov = json.load(open(prov_path, encoding="utf-8")) if prov_path.exists() else []
    lines = [
        f"# GitHub Actions 탐색 결과 ({dst.name})", "",
        "결과는 러너(ubuntu-latest, 4 vCPU)에서 나온 것이며 로컬 실행과 같은 엔진 커밋·인자일 때만 비교한다.",
        "값은 평가 함수 점수이지 승률이 아니다(AGENTS.md).", "",
        "| 시나리오 | 커밋 | solve | rolls | side | extra | 러너 | run_id |", "|---|---|---|---|---|---|---|---|",
    ]
    for p in prov:
        lines.append(
            f"| {p.get('scenario')} | {str(p.get('commit'))[:7]} | {p.get('solve')} | {p.get('rolls')} | "
            f"{p.get('side')} | {p.get('extra')} | {p.get('runner')} ({p.get('cpus')} cpu) | {p.get('run_id')} |"
        )
    (dst / "README.md").write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"unpacked into {dst} ({len(prov)} shards)")


if __name__ == "__main__":
    cmd, *rest = sys.argv[1:] or ["help"]
    if cmd == "pack" and len(rest) == 2:
        pack(*rest)
    elif cmd == "merge" and len(rest) == 2:
        merge(*rest)
    elif cmd == "unpack" and len(rest) == 2:
        unpack(*rest)
    else:
        sys.exit(__doc__)
