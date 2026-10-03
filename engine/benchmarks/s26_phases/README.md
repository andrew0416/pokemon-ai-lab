# S26h: phase costs and clean runtime controls

No algorithm change. Parent S22 / 70d5b0b463dc2cab2deb365c67ffcb2e2e7f95b8.
Eight bounded workloads: the original six plus cadence=1 on stats-w4-d2 and
narrow-w1-d3, exposing more compiled snapshots of the same finite games.

Each workload has eight speed comparisons: original/original A/A; original/new
OFF; new OFF/new OFF A/A; new OFF/index; new OFF/owned; new OFF/compressed checks;
new OFF/all four; owned/owned+delta (required dependency held fixed).
Runtime A/A controls use identical APIs and parameters; only reporting labels differ.
All timing pairs use the same **uninstrumented** generic x86-64 release binary,
nine alternating ABBA/BAAB blocks, original-tree certificates and a common gap.
Intervals describe within-run paired variation, not machine-to-machine uncertainty.
Nonconverged pairs are censored. Local modes never emit performance numbers.

`experiment-phase-cost` is separate, default OFF. Every instrumentation call is
guarded at compile time. Its release executable refuses headline speed mode;
the speed executable refuses profiling. No profile code enters engine/core.
Five rotated repetitions give phase diagnostics only, not speedup claims.
Caller wall spans form a nested ledger: exclusive costs sum exactly to compute;
inclusive rows overlap and must not be added. The admission remainder includes
legal-action queries, evaluation, observation formatting, raw tree construction,
and associated allocation. The transition row measures synchronous serial calls
or main-thread batch dispatch/wait, including scheduling. No worker CPU sum.
Compiler includes metadata cloning and full validation. Kernel includes full or
incremental layout/payoff preparation. CFR excludes nested certificate/filter
time. Solver remainder covers setup and internal cleanup. Compute remainder
covers adapter/root setup, bookkeeping, and other internal object destruction.
Returned-result destruction is separately measured after diagnostic inspection;
it is excluded from headline compute timing, matching the parent convention.

Full Tree/Solution/diagnostic streaming FNV-1a-128+length witnesses are compared
across instrumented and uninstrumented builds, and every profiled repetition.
They are regression witnesses, not cryptographic proof; downloaded evidence is
SHA256 pinned. Phase calls must reconcile with solve/iteration/assessment counts.
The collector itself tests nested exclusive accounting, thread isolation, errors,
and unwinding. Existing independent full-tree LP/BR checks remain the oracle.

This study does not collect per-variant allocation or peak memory, prove actual
non-AVX2 hardware execution, cover every future gimmick, or assess playing strength.
