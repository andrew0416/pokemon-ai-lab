# Incrementally encoded private histories (S26j / S25)

Parent: S24 `d90c80b4001e43807d404454280f1511329ef952`.
S24 measured leaf history-key formatting at 11.00–13.90% of diagnostic compute.
The experimental `pipeline.encoded_history` setting defaults to false. No engine/core
change, AVX requirement, observation cache, evaluator cache or learned policy is added.

## Representation and semantic contract

OFF retains the original owned `Vec<Memory>` for each player. ON retains the exact
`player:[Memory, ...]` key. To extend it, copy all but the internally produced final
ASCII `]`, then append only the own action and the new public/private observation.
The existing derived `Memory<S>` Debug implementation formats both borrowed `&str`
and owned `String` tokens. There is no duplicated escaping implementation or history
hash that could merge distinct information sets. Root keys still use Cursor memory.

The full growing tree, information IDs, semantic menus, public-history ordering,
private types, policy, original-tree certificate, callback order and work counters
must remain identical for the same settings. Domain callbacks can be stateful;
none is skipped, reordered or memoized. Pending admission rollback is unchanged.
History mode is rejected for fixed-tree solves. Legacy scalar features do not compile
the encoded variant. Enabling the experiment also changes the private enum layout in
the OFF growing path; clean same-binary ratios do not prove zero cross-version OFF
overhead. Production remains unchanged and this flag remains opt-in.

## Correctness

The Cursor differential covers every prefix through 128 decisions, independent
siblings, quotes, control bytes, Unicode, empty and syntax-looking observations.
An observed domain records all callbacks and prior calls across 36 combinations of
switch closure, transition caps, partial/full admission and pipeline settings.
The CI observed-game corpus adds bijective escaped semantic labels and new seeds,
then compares the entire OFF/ON JSON for all eight inherited settings, including warm
initialization and regret rules. Independent exhaustive BR and SciPy LP validate
the exported game and certificate; capped nonconvergence is reported separately.
S24 intermediate snapshot, payoff contraction and append-only guards are retained.

## Measurements

Eight workload settings from six bounded engine fixtures, not eight independent games:
stats 1/2/4 worlds depth 2, pivot and sand depth 1, narrow depth 3, plus cadence 1 on
stats 4-world and narrow workloads. Each of two pairs has a true same-path A/A:

- `new-off` vs `history`: new option alone.
- `all-four` vs `all-history`: new option added to the prior four options.

There are 16 conditions, 32 comparisons and 9 ABBA/BAAB blocks each. Positive
reduction means faster. Both pairs require unchanged results and work, not just an
equal iteration cap. No win rate is measured. Local performance runs are refused.

Speed, phase clocks, intermediate shadow checks and allocation counters use distinct
binaries with `-C target-cpu=x86-64`; diagnostic features refuse `--measure`.
Five rotated diagnostic captures per configuration: 8 x 5 x 5 = 200. Formatting new
events now occurs in `history_copy`, so compare `history_copy + history_keys` plus
menu work before attributing a phase reduction to eliminated work. Headline speed
includes internal builder teardown but excludes inputs, JSON, pool construction and
returned-result drop. Separate memory processes measure 8 x 5 x 3 = 120 captures;
requested Rust heap bytes and per-child RSS describe different scopes.

QEMU's Nehalem model must report AVX=false, AVX2=false and SSE2=true before executing
two engine cases with five configurations plus the legacy scalar path. Complete native
and emulated results must match. This is emulated execution, not old hardware timing.

All evidence is exploratory and bounded; this does not prove correctness for every
future game mechanic, benchmark absolute speed on user hardware or strength gains.
