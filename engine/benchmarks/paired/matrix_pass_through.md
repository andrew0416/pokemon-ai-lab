# P14 independent exact comparison

The same immutable source is compiled twice: common P8g/P8c/P9/P10/P8d, then
that exact set plus search-only `experiment-matrix-pass-through`. P8e/P8f,
P11/P12/P13, and every observer are absent from the timing binaries. The
existing generic x86-64, one-thread narrow workload and timing harness are
unchanged. Each case uses ten alternating measured AB/BA pairs; peak RSS is a
separate two-pair pass. Compilation and observer execution are never timed.

Both arms produce fresh 6,184-case oracle/turn/search evidence. Exact raw output
is required; no instruction-representation exception is permitted. Full release
core/scenario/search regressions run before measurement, even with cached
dependencies. Verified binaries cannot substitute for fresh test execution.

Bounded debug opt0 proof covers dense/compact storage, runtime OFF/ON and plain/
observer (eight variants). The single public test records 51 complete cases,
including both slots, chance policies, one/two threads, dominance and lazy
switches, depth three, terminal/unsupported states, resume/replacement, NaN
evaluator fallback and exact turn instruction/rollback records. Observer unit
tests additionally compare 1,378 matrix input/output records against the frozen
source048 scalar reference. NaN masks/payloads, signed zero, infinities, empty
axes, malformed dimensions and existing panic behavior are retained.

The allocator proof is an isolated subprocess. P14a removes reconstruction
allocations. P14b measures only the choice clone segment after the values vector
already exists; the values vector and Nash allocations remain. Allocation
removal is not a whole-search speed claim.

Only the two compact observer arms get an extra release observer build. They
run coaching/sand through a separate wrapper with the immutable harness's
analysis semantics and compare exact output/work. OFF pass counts are zero;
ON must activate at least one fast path. Each kind obeys calls=pass+fallback,
`b_calls` is conserved, and `a_calls_OFF=a_calls_ON+b_passthrough_ON` because
P14b avoids calling the P14a filter. If a real workload has zero P14b passes it
is retained honestly; bounded tests must still demonstrate both paths and both
fallbacks. None of these counters enter timing builds.
