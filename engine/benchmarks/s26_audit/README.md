# S26i audit definitions

Frozen parent S23. Four separate build modes: uninstrumented speed, phase-only
diagnostics, expensive intermediate snapshot shadows, and allocation diagnostics.
Every diagnostic is default OFF and refused by headline speed mode.

The phase ledger now splits menu generation, observations, phase queries, history
copy/append, history Debug keys, public/private boundary registration, evaluation,
and frontier insertion. Exclusive wall costs remain exactly additive; per-call
clock overhead is present only in this diagnostic build. Worker CPU time is not
summed into caller wall time.

Speed confirmation uses the two many-snapshot conditions (15 and 25 solves),
16 comparisons including true original/new-OFF AA, 15 alternating paired blocks.
This is a second runner session, not proof on all computers or all game sizes.

Allocation mode launches a fresh process for each workload/configuration/repeat
(8 × 7 × 3 = 168). A System-delegating allocator counts requested Rust bytes and
calls across worker threads. Counters are never reset while allocations survive;
only the peak baseline is reset immediately before compute. Reallocation counts
positive requested growth, not internal allocator traffic or usable size. Process
RSS is /usr/bin/time's peak for that one child and includes load/pool/runtime.
Result-drop accounting subtracts allocations held by diagnostic metadata. These
measurements are not speed measurements. Cache shape reports retained empty
buckets, capacities and rank slots processed; shape tracking itself allocates no
trace vectors. Performance builds contain none of this tracking.

Snapshot audit checks the private append-only/old-nonterminal-immutable contract,
then compares every successful incremental compilation with a fresh full compile.
It compares incremental sequence gradients under four policies and original-tree
BR gaps at each cache update. Conservative fallback differences are allowed.
Generated compiler tests use 64 seeds and up to 24 replacements each, including
canonical ID remapping, changing players/actions, scale changes and tiny chance.
The independent oracle uses a new seed and 64 growing games plus 29 fixed games,
with partial budgets and atomic pending-batch failure. These are generic contracts,
not a claim that every possible future Pokemon gimmick has already been tested.

Portability uses QEMU user emulation with the Nehalem CPU model. A compiled CPUID
probe must observe AVX=false, AVX2=false, SSE2=true. Native and emulated results
must match for two bounded engine workloads × three settings and a legacy scalar
tree CLI. This is explicitly emulated execution, not physical old-CPU testing and
not a performance comparison. Nothing is installed on the user's desktop.
