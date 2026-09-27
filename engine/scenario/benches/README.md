# Turn throughput bench (`benches/turn.rs`)

What one turn of lab-engine costs on real positions (Opus GG unit GG-turn-throughput).

```bash
cd engine && cargo bench -p lab-scenario --bench turn            # all positions (~50 s)
cd engine && cargo bench -p lab-scenario --bench turn -- psy-cona # name filter
LAB_BENCH_MS=2000 cargo bench -p lab-scenario --bench turn        # longer batches
LAB_BENCH_METRICS=sweep,median cargo bench -p lab-scenario --bench turn   # some columns only
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
warm-up call.

Machine: AMD Ryzen 5 3500X (6 cores, 32 GB), Windows 10 Home 19045, rustc 1.98.1
(x86_64-pc-windows-msvc), `bench` profile = workspace `release` (opt-level 3, `debug = 1`, no
LTO, 16 codegen units). Other sessions were compiling and testing on the machine at the same
time (load up to 100 %); a single run can be 2x slower than a quiet one. Comparisons are
therefore made by running the two builds' bench executables alternately (three rounds here) and
keeping each cell's fastest value.

## Before / after (`52f1379` -> `c649d53`, alternating runs)

| position | legal | sample µs | median ms (n) | extremes ms (n) | full ms (n) | sweep ms (n) |
|---|---|---:|---:|---:|---:|---:|
| cc-lib-sand-owen-vs-coaching-panda | 178x138 | 43.8 -> 25.9 | 1.083 -> 0.277 (12) | 6.885 -> 1.429 (52) | 168.3 -> 48.3 (1026) | 1.194 -> 0.356 (256) |
| cc-lib-coaching-panda-vs-psy-sand-udon | 138x94 | 42.7 -> 24.9 | 0.410 -> 0.151 (5) | 1.776 -> 0.533 (16) | 249.4 -> 110.7 (522) | 0.292 -> 0.107 (255) |
| cc-lib-psy-cona-vs-sand-owen | 78x178 | 48.8 -> 28.5 | 0.618 -> 0.251 (6) | 2.937 -> 1.005 (16) | 294.7 -> 136.2 (297) | 2.253 -> 0.716 (253) |
| cc-lib-psy-sand-udon-vs-balance-ddee | 94x138 | 47.4 -> 27.9 | 0.427 -> 0.176 (5) | 3.137 -> 0.969 (33) | 881.7 -> 255.6 (7246) | 1.811 -> 0.550 (255) |
| cc-lib-balance-ddee-vs-crown-cecil9 | 138x166 | 44.4 -> 26.9 | 0.550 -> 0.228 (4) | 3.528 -> 1.370 (16) | 605.3 -> 308.3 (725) | 1.510 -> 0.440 (255) |
| cc-lib-crown-cecil9-vs-perish-mrada | 166x214 | 38.0 -> 21.4 | 0.337 -> 0.109 (4) | 1.063 -> 0.296 (12) | 18.8 -> 4.8 (113) | 0.649 -> 0.190 (256) |
| spread-damage | 80x64 | 50.7 -> 30.7 | 11.320 -> 2.723 (114) | 301.9 -> 102.9 (3060) | skipped | 2.080 -> 0.697 (256) |
| o48-flash-fire-spread | 8x4 | 37.6 -> 21.3 | 0.487 -> 0.153 (4) | 1.182 -> 0.296 (8) | 5.6 -> 1.3 (34) | 0.244 -> 0.089 (32) |
| o21-wide-guard | 4x4 | 32.9 -> 17.8 | 0.161 -> 0.075 (2) | 0.431 -> 0.139 (4) | 2.4 -> 0.6 (16) | 0.287 -> 0.107 (16) |

Outcome counts are the same before and after (they are the same outcome lists). Not changed by
the unit, and unchanged within noise: `legal` (3.1–166 µs for both sides, the same in both builds)
and `clone` (145–150 ns). Speed-ups: Median 2.1–4.2x, Extremes 2.6–4.8x, Full 2.0–4.4x, the Median
sweep 2.7–3.4x, `sample_turn` 1.65–1.85x.

End to end, same machine and load, single thread: `lab-plan cc-lib-psy-cona-vs-sand-owen.json
--side p1 --rolls median --solve nash --threads 1` (5,940 enumerations, 134,891 nodes) 21.98 s ->
5.74 s; `lab-rollout` on the same scenario, `--games 2 --seed 20260926 --threads 1`, 30.0 s ->
8.5 s. Both print the same results before and after (only the elapsed times differ).

## Per commit (previous commit -> this, alternating runs of the two builds)

| commit | change | sweep psy-cona ms | median sand-owen ms | extremes sand-owen ms | sample sand-owen µs |
|---|---|---:|---:|---:|---:|
| `22647a6` | hash only the set volatiles | 2.32 -> 1.50 | 1.09 -> 0.67 | 6.95 -> 3.99 | — |
| `6325e08` | merge positions by a precomputed hash | 1.50 -> 0.94 | 0.76 -> 0.39 | 4.25 -> 2.09 | 43.4 -> 39.9 |
| `11b514d` | inline slot lists for `alive_slots` / `all_alive` | 0.96 -> 0.81 | 0.38 -> 0.35 | 2.04 -> 1.85 | 39.2 -> 32.6 |
| `45a695a` | replay runs without re-deriving the battle | 0.82 -> 0.77 | 0.35 -> 0.36 | 1.93 -> 1.79 | 33.1 -> 33.0 |
| `fd6ae40` | reuse run buffers across positions and sample stages | 0.77 -> 0.76 | 0.36 -> 0.33 | 1.79 -> 1.71 | 32.5 -> 29.3 |
| `e3ee043` | allocation-free roll, action pick and residual sort | 0.77 -> 0.73 | 0.33 -> 0.34 | 1.74 -> 1.72 | 29.2 -> 27.6 |
| `c649d53` | cheaper Update speed sort and item reads | 0.74 -> 0.70 | 0.34 -> 0.31 | 1.75 -> 1.60 | 27.8 -> 26.0 |

Each row is its own alternating comparison, so a row's "before" need not equal the previous row's
"after". `45a695a` mainly helps positions with many runs (Full 282 -> 269 ms on
psy-sand-udon); `e3ee043` Full 261 -> 251 ms there. Tried and dropped: packing `VolatileState`
into one word for equality and hashing (no measurable change, sweep 0.77 -> 0.79 ms).

## Where the time goes (profiles)

`samply` needs the Windows Performance Toolkit (`xperf`) and administrator rights on Windows,
and `cargo flamegraph` needs DTrace; neither was available. The profiles come from a small
in-process sampler (kept out of the repository): the workload runs in a worker thread, the main
thread suspends it every 0.5 ms (`SuspendThread` / `GetThreadContext`), walks its stack with
`RtlLookupFunctionEntry` / `RtlVirtualUnwind`, resumes it, and symbolizes the addresses afterwards
with dbghelp, inline frames included (the release profile has `debug = 1`). Workloads: the Median
sweep over legal pairs of the six library lead turns (as `sweep`), and `sample_turn` on the six
scenario turns. Shares are of all samples; "incl." counts a function once per stack.

Before (`52f1379`, Median sweep 1.61 ms per enumeration in the sampler):

1. Merging identical positions: SipHash of whole `State`s, 55.6 % incl. (37 % of all samples in
   `HashMap` growth rehashing every stored state; the 4 x 111 `VolatileState` entries per state,
   6 fields each, were 36 % incl.).
2. Heap allocation and free: `RtlAllocateHeap` 12.9 % + `RtlFreeHeap` 6.4 % incl. (plus ntdll heap
   internals without symbols, about 5 %). In `sample_turn` 22.9 % + 10.0 %.
3. `State` copies (`memmove` / `memcpy` 8.6 % self): map entries moved on growth, two or three
   clones per new end position.
4. `Vec<SlotRef>` from `alive_slots` / `all_alive`: 11.4 % incl. of `sample_turn` (the Update
   event's Symbiosis check asked for a side's slots per active after every action and hit).
5. `Battle::new` per run (history readers, suppression check, Speed snapshot): 5.9 % of
   `sample_turn`, about 1–5 % of enumeration.

After (`c649d53`, Median sweep 0.45 ms per enumeration in the sampler, `sample_turn` 29 µs vs 86 µs
before in the same harness): the merge (`Merger::add`) is 32 % incl., of which hashing the state
18 % and comparing equal states 3–4 %; heap 13 % incl. (mostly the move code's small `Vec`s in
`hit_loop` / `spread_move_hit` and the outcomes' `Box<Slot>` from `diff::instructions`); `State`
copies 12 %; the rules code (`run_move` 37 % incl.) is the rest.

`legal_joint_actions` (not on the search's hot path: once per side per node) spends 63 % in
`check_side` for every raw joint action and 30 % in its quadratic de-duplication; left as is.

## Behaviour checks

Every commit left the engine's output unchanged, checked two ways: the full test suite (982
tests, fixtures included) and a digest over all 925 files in `oracle/scenarios` (2,470 decision
positions): per position the outcome list (order, probability bits, instructions, suspension) at
Median and Extremes, and at Full where an exact `.turn.json` fixture exists (2,434),
`sample_turn` with 64 samples and with 1 sample for three seeds, and both sides'
`legal_joint_actions`; 22,164 lines, byte-identical to the `52f1379` build at every commit.
