"""Fits linear evaluation weights to `lab-plan --dump-children` rows (WORKPLAN S12).

Usage:
    python engine/scripts/fit_eval.py <rows.jsonl>... [--ridge 1.0] [--out weights.json]

Each row is `{"features": [...], "target": v, "p": probability, ...}` where `features` are
`lab_engine::eval::features` from our side and `target` is the position's next-turn matrix-game
equilibrium value (the bootstrap target). Rows are weighted by `p`. The fit is ridge least
squares without an intercept (a symmetric position must score 0). The output JSON has
`weights` by feature name, ready for `lab-plan --eval file:<weights.json>`, plus the fit
diagnostics. The current hand-set weights (`Heuristic::WEIGHTS`) are printed next to the fit.
"""
import argparse
import json
import pathlib

import numpy as np

NAMES = [
    "alive", "hp_fraction", "sleep", "freeze", "paralyze", "burn", "poison", "toxic",
    "offensive_stages", "defensive_stages", "accuracy_stages", "confusion", "leech_seed",
    "substitute", "taunt", "encore", "perish_song", "yawn", "stall", "tailwind", "screens",
]
HEURISTIC = [30, 100, -45, -50, -18, -14, -10, -16, 9, 6, 4, -12, -10, 12, -6, -8, -20, -25, -10, 12, 8]
# Rows written before the `stall` feature existed have 20 entries: insert a 0 for it.
STALL_INDEX = NAMES.index("stall")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("rows", nargs="+")
    ap.add_argument("--ridge", type=float, default=1.0)
    ap.add_argument("--out", default="")
    a = ap.parse_args()
    X, y, w = [], [], []
    for path in a.rows:
        for line in pathlib.Path(path).read_text(encoding="utf-8").splitlines():
            if not line.strip():
                continue
            r = json.loads(line)
            feats = list(r["features"])
            if len(feats) == len(NAMES) - 1:
                feats.insert(STALL_INDEX, 0.0)
            X.append(feats)
            y.append(r["target"])
            w.append(r.get("p", 1.0))
    X = np.array(X, dtype=float)
    y = np.array(y, dtype=float)
    w = np.array(w, dtype=float)
    n, k = X.shape
    assert k == len(NAMES), k
    # Weighted ridge: (X'WX + λI) β = X'Wy
    sw = np.sqrt(w)[:, None]
    Xw, yw = X * sw, y * sw[:, 0]
    beta = np.linalg.solve(Xw.T @ Xw + a.ridge * np.eye(k), Xw.T @ yw)
    pred = X @ beta
    resid = y - pred
    heur = X @ np.array(HEURISTIC, dtype=float)
    rmse = float(np.sqrt(np.average(resid**2, weights=w)))
    rmse_h = float(np.sqrt(np.average((y - heur) ** 2, weights=w)))
    support = (X != 0).sum(axis=0)
    print(f"rows {n}, target mean {y.mean():+.1f} sd {y.std():.1f}; fit RMSE {rmse:.1f} vs heuristic RMSE {rmse_h:.1f}")
    print(f"{'feature':>18} {'fit':>9} {'heuristic':>10} {'nonzero rows':>13}")
    for name, b, h, s in zip(NAMES, beta, HEURISTIC, support):
        flag = "" if s >= 10 else "  (few rows: unreliable)"
        print(f"{name:>18} {b:>+9.1f} {h:>+10.1f} {s:>13d}{flag}")
    if a.out:
        out = {
            "weights": {name: float(b) for name, b in zip(NAMES, beta)},
            "rows": int(n),
            "ridge": a.ridge,
            "rmse": rmse,
            "heuristic_rmse": rmse_h,
            "support": {name: int(s) for name, s in zip(NAMES, support)},
            "sources": a.rows,
        }
        pathlib.Path(a.out).write_text(json.dumps(out, indent=1), encoding="utf-8")
        print("wrote", a.out)


if __name__ == "__main__":
    main()
