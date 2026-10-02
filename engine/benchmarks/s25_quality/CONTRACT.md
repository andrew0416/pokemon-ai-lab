---
id: EXP-S25b-search-quality-20261002
status: implementation-validated-ci-pending
kind: experiment-contract
parent_source: c54becc9fb7bb9101cac363f389d4a84792016ab
production_adopted: false
default_enabled: false
---

# S25b: completed-policy quality versus elapsed time

This is a pilot, not a playing-strength result. The immutable plan is `plan.json`.
All seven original inputs are retained without move, party, HP or legality edits.
Five are small mechanics fixtures (not tournament-legal team claims). The two
library team cases are coaching and sand, using the existing scenario assumptions.
Position index 0 is explicit; this is not belief-state averaging. Initial-state
count, selected mass, full root action IDs and input hashes are recorded.

Both arms use the same executable and evaluator, normal-turn depth target 2,
Sensible pruning, uncapped chance outcomes, one CPU, generic x86-64 and KEEP-R1
features. P1e/P7d/P7h and timing observers are OFF. Engine transitions here use the
existing flat search API; this is not the earlier factored-Full throughput test.
Median reduces damage rolls only; it does not mean all randomness is deterministic.

## Algorithms and scoring

- Baseline: existing mixed depth-1 solver followed by existing deep-nash beams
  1, 2, 4, 8, all (duplicates removed). Child transposition tables persist between
  stages. Repeated shallow work is charged. No unpublished partial matrix is used.
- Selective: uniform-prior S25a, 20,000 RM+ iterations per matrix, 0.01 outer local
  gap target, 200,000 stored-node ceiling and 2M transition ceiling. It publishes
  only after every affected ancestor has been backed up. Initial incomplete
  matrices produce no answer. Observer I/O is inside elapsed time.
- Both search arms retain their own existing inner-solver tolerances (baseline
  root 0.01 / lazy child 0.05, selective inner 0.001); these are algorithm settings,
  not a bitwise-equivalence test. Baseline terminal depth bonus is disabled only
  by a new default-OFF benchmark feature and a runtime flag whose default is true.
- Existing baseline horizon behavior is preserved: pending switch states may be
  heuristically valued at depth zero, and some switch continuations use maximin.
  S25a resolves free switches by mixed games before the horizon. The comparison
  tests the resulting algorithms, not a claim of identical evaluated game trees.
- A separate all-action recursion resolves chance and switches over two normal
  turns. It shares EngineDomain and RM+, but no PUCT/beam/DO/cache code. It uses
  200,000 iterations / 0.0001 tolerance and propagates row-security / column-upper
  bounds through every chance and matrix node. Root cell intervals and maximum
  local solver gap expose unresolved numerical error. These are floating-point
  numeric bounds, not a formal directed-rounding proof or an independent engine.
- Every completed policy is lifted to the same complete root action order, then
  evaluated on the common reference matrix: full-root best-response gap, interval
  uncertainty, value estimate error, policy TV, and modal-choice agreement. Different
  modes/supports can describe equally good equilibria; agreement is diagnostic.
  This root matrix metric is not full-game exploitability or win probability.

## Budget and resource contract

Internal monotonic timestamps select the last completed snapshot at or before
each frozen cutoff. No post-cutoff result can replace a pre-cutoff policy. There is
no uniform-policy fallback. A missing policy remains missing, never zero error.
The baseline finite-turn-budget option is not used as a hard global bound.
Transition curves select completed policies at common count cutoffs, with both
turn and switch calls weighted 1; they are observed curves, not hard work-capped
baseline runs. Incomplete final work and uncompleted baseline stages are not
represented as completed transition snapshots.

An external watchdog kills only its owned process 0.25 seconds after the last
cutoff (60 seconds for setup). Enumeration/RM+ are not internally preemptible.
Out-of-time snapshots remain in raw JSONL but are ineligible. CPU and peak RSS are
whole-process OS `wait4` results, including setup/cleanup/possible final overrun;
they are not per-cutoff measurements. Address space is limited to 4 GiB.

One warmup per arm is excluded. Three paired trials use fixed seeds 1/7/19, AB/BA/AB
order, sequential on each case's VM and a single pinned CPU. Other cases run on
different VMs; cross-case process time is not a direct machine-speed comparison.
Reference gets 120 seconds and 2M transitions. Failure, timeout, unsupported cells
and truncated output are retained. An explicit later domain error invalidates
earlier strategies for that trial. Expected deadline kills preserve prior completed
strategies; other process failures do not. Large coaching/sand cases have no
full-depth reference in this pilot and cannot establish relative decision quality.

## Gates and source scope

Fresh default-OFF library tests, feature-ON tests, observer interruption tests,
an independent-recursion real-engine comparison, and scorer failure regressions
precede remote timing. These are scoped search checks, not all-engine regression
or fresh full500/6184 mechanics certification. The source manifest records actual
resolved features, source SHA, parent, toolchain, input and binary hashes.
Only an isolated `codex/s25b-search-quality-20261002` branch is published. Existing
production changes, prior experiment evidence and KEEP-R1 membership are untouched.
