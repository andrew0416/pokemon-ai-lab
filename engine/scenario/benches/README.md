# Turn throughput bench (`benches/turn.rs`)

What one turn of lab-engine costs on real positions (Opus GG unit GG-turn-throughput).

```bash
cd engine && cargo bench -p lab-scenario --bench turn            # all positions (~50 s)
cd engine && cargo bench -p lab-scenario --bench turn -- psy-cona # name filter
LAB_BENCH_MS=2000 cargo bench -p lab-scenario --bench turn        # longer batches
```

Positions: the six committed library lead turns (`oracle/scenarios/cc-lib-*.json`, VGC Reg
M-C, position 0) and three hand scenarios with spread moves (`spread-damage`: Rock Slide +
Hyper Voice + two single-target attacks; `o48-flash-fire-spread`: Heat Wave into Flash Fire;
`o21-wide-guard`: Rock Slide + Earthquake into Wide Guard). The joint action is the scenario's
own `turn`.

Columns: `legal` = `legal_joint_actions` counts p1 x p2 and the time for both calls; `clone` =
one `State<2>` clone; `sample` = one `sample_turn(.., 1, seed)` call with a fresh seed (what
`lab-rollout` does per turn; `check_turn` and the instruction diff included); `median` /
`extremes` / `full` = one `enumerate_turn_with` at that `RollMode`, with the outcome count in
parentheses. `spread-damage` has no `full` (its exact distribution does not fit in memory,
WORKPLAN F18). `sweep` = `enumerate_turn_with` at Median averaged over up to 256 legal
joint-action pairs (every k-th of p1 x p2; what `lab-plan --solve nash` spends its time on),
with the number of pairs enumerated. Each number is the fastest of five batch means after a
warm-up call. `LAB_BENCH_METRICS=sweep,median` runs only some columns.

Machine: AMD Ryzen 5 3500X (6 cores, 32 GB), Windows 10 Home 19045, rustc 1.98.1
(x86_64-pc-windows-msvc), `bench` profile = workspace `release` (opt-level 3, `debug = 1`, no
LTO, 16 codegen units). Other sessions were using the machine at the same time; repeat runs
move by up to about 10 %.

## Baseline (`52f1379`)

| position | legal | legal µs | clone ns | sample µs | median ms (n) | extremes ms (n) | full ms (n) |
|---|---|---:|---:|---:|---:|---:|---:|
| cc-lib-sand-owen-vs-coaching-panda | 178x138 | 126.1 | 153 | 45.8 | 1.077 (12) | 7.076 (52) | 168.3 (1026) |
| cc-lib-coaching-panda-vs-psy-sand-udon | 138x94 | 81.1 | 143 | 45.2 | 0.452 (5) | 1.888 (16) | 266.9 (522) |
| cc-lib-psy-cona-vs-sand-owen | 78x178 | 102.9 | 147 | 51.1 | 0.648 (6) | 3.102 (16) | 301.1 (297) |
| cc-lib-psy-sand-udon-vs-balance-ddee | 94x138 | 91.0 | 147 | 50.9 | 0.458 (5) | 3.338 (33) | 930.3 (7246) |
| cc-lib-balance-ddee-vs-crown-cecil9 | 138x166 | 122.8 | 149 | 47.4 | 0.647 (4) | 3.644 (16) | 616.8 (725) |
| cc-lib-crown-cecil9-vs-perish-mrada | 166x214 | 165.0 | 147 | 39.9 | 0.350 (4) | 1.060 (12) | 18.9 (113) |
| spread-damage | 80x64 | 45.5 | 146 | 53.2 | 11.681 (114) | 323.6 (3060) | skipped |
| o48-flash-fire-spread | 8x4 | 4.3 | 146 | 39.4 | 0.491 (4) | 1.181 (8) | 5.4 (34) |
| o21-wide-guard | 4x4 | 3.3 | 144 | 33.5 | 0.177 (2) | 0.453 (4) | 2.6 (16) |

`sweep` at the same commit (ms per enumeration, pairs enumerated; a second run under heavier
load from other sessions was up to 2x slower, which is why the final comparison below runs the
two builds alternately):

| position | sweep ms (n) |
|---|---:|
| cc-lib-sand-owen-vs-coaching-panda | 1.607 (256) |
| cc-lib-coaching-panda-vs-psy-sand-udon | 0.402 (255) |
| cc-lib-psy-cona-vs-sand-owen | 2.578 (253) |
| cc-lib-psy-sand-udon-vs-balance-ddee | 2.266 (255) |
| cc-lib-balance-ddee-vs-crown-cecil9 | 1.730 (255) |
| cc-lib-crown-cecil9-vs-perish-mrada | 0.719 (256) |
| spread-damage | 2.482 (256) |
| o48-flash-fire-spread | 0.298 (32) |
| o21-wide-guard | 0.365 (16) |
