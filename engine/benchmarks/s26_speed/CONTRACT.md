# S26c speed-only comparison

This harness measures the already verified S26c source `26999d068dd541ac0f2cfcdafc8a091ce4427e12`.
Only benchmark files and a benchmark binary declaration are added. Core simulation,
exhaustive builder, growing builder and CFR/BR kernels are byte-identical to that source.

One optimized generic x86-64 binary contains both paths; there is no per-variant rebuild.
One GitHub runner executes serially, pinned to one available logical CPU, with one thread.
The measure is build + CFR time until a decision result is available. Scenario loading,
setup turns, JSON construction/printing, diagnostics and destruction of the final returned
tree are excluded. Internal construction/copying/destruction inside the search remains
part of its measured work. Every repetition starts a fresh search: reuse across successive
game turns is not claimed.

Four bounded cases are predefined: two-turn stat-observation fixture with one and two
hidden worlds; one-turn U-turn/Eject Button; and one-turn sand with damaging attacks and
support abilities. The first two are small, status-heavy correctness fixtures rather than
a representative tournament workload. Do not generalize their latency to full VGC menus.
All use Full chance rolls, all legal actions, the same Heuristic, generic x86-64, maximum
4,096 CFR iterations, check interval 32 and tolerance 0.1. The full selected horizon must
finish and meet tolerance before its timing is accepted. Partial trees must meet the
same tolerance for their own fixed-leaf surrogate; this does not equal full-depth work.

The exhaustive path is the paired baseline. Growth budgets 1/2/4/8 are reported separately
from growth continued to the complete requested horizon. Three selection seeds (1/7/31)
are used where selection occurs. Forced switch closure is deterministic, so pivot and
sand use one seed. Every complete-growing result must match baseline node count,
information-set count, transition count, root value and root policy (within 1e-7).
The input games, evaluator, horizon and solver tolerance do not change between paths.

After untimed validation/warmup and calibration, each treatment has seven paired blocks.
Order alternates ABBA and BAAB; treatments rotate between blocks. A calibration targets
80 ms per sample, capped at 64 independent solves, minimum one. The metric is mean ns
per solve within a sample; medians and ratios are then computed over paired blocks.
This excludes process startup and does not mislabel batch means as individual tail
latencies. A 95% paired bootstrap interval describes within-runner block variability;
it is not a cross-machine confidence interval. No case is silently dropped for errors
or timeouts. Raw samples, work counts, solver status and CPU details are retained.

Lower time for a partial tree can arise from less continuation work. Full-horizon growth
can instead be slower because it clones candidate trees and restarts CFR after admissions.
These are distinct comparisons. This task measures neither wins nor playing strength.

Local `run.py --check` checks correctness/benchmark eligibility without timing. `--measure`
is invoked by the GitHub workflow via `run.py` only. The feature remains default OFF and
no production planner is changed.
