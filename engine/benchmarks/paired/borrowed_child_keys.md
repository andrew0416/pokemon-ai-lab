# P13 borrowed child keys: independent CI

The same immutable source `5d2d3581150b4464cc6586fd0c818be4b4a15d46` is compiled twice. Baseline is old4 (hurt readers, prepared turn, leaf ending states, compact Volatiles) plus P8d replay action keys. Candidate adds `lab-search/experiment-borrowed-child-keys`, forwarding the core helper flag. P8e, P8f, P11 and P12 stay OFF. This measures the incremental effect of P13 after old4+D; prior common6/7 logic receipts do not substitute for this run.

## Required order

1. Fresh two-variant 6,184-case agreement, with the original frozen corpus, probes, selection and oracle comparators. Turn/search streams require exact raw bytes/digests. There is no P8e instruction exception. Prepared/leaf observers are present only in agreement/diagnostic builds.
2. Fresh release regressions for core, scenario and search on both arms, immutable source/manifest checks, strict actual Cargo feature fingerprints, original compact probe checks, and existing prepared/leaf coexistence diagnostics.
3. New owner gate: eight fresh bounded debug opt0 builds (dense/compact × OFF/ON × plain/observer). Each emits 47 public records; all byte-identical. Four observer configurations also execute the named collision, capacity, error-order and capture tests, isolated real-allocation subprocesses, and five exact private records. The tests contain 36 capture subcases per runtime across the storage axes. Debug fingerprints bind the actual test artifacts, including the core/search forwarding flags. This reduces unnecessary release compilation while retaining the full bounded proof.
4. Two separate release observer builds run public search on the frozen coaching and sand positions with common5. Their complete result/work stdout must match exactly. Counters must show positive owned captures OFF, zero owned captures and positive borrowed queries ON, `OFF.key_captures == ON.borrowed_queries`, unchanged job captures/seen hits, and positive indexed-seen insertion. Observer stderr and binary/fingerprint hashes are preserved. These runs are diagnostic and their duration is not performance evidence.
5. Observer-free, generic x86-64 release timing on one VM: coaching/sand depth2, one thread, 10 alternating AB/BA pairs, followed by two separate RSS pairs. Frozen `harness.rs`, `run.py`, `memory.py`, suites and inputs remain byte-identical.

The new `borrowed_child_keys_probe.rs` is an independent observer wrapper derived from the immutable timing harness. `ci_bench.rs` remains the unchanged timing harness. No production reference selector or runtime observer is added to timing binaries.

## Fail-closed evidence

The owner gate requires source SHA and ten implementation-file pins, default-OFF declarations and independent observer declarations. It rejects cached test results, missing/filtered/ignored named tests, missing allocator execution, malformed or missing records, storage/observer/OFF/ON differences, wrong/extra actual features, missing public activation and changed outputs/counters. Failures retain commands, stdout/stderr and a failed receipt. Evidence paths are under `ci-results/borrowed-child-keys-validation` and `ci-results/fingerprints`. Debug and release fingerprint copies are distinct.

The current private Suspension limitation remains: existing complete Eq/Hash is used, real different suspended queues and public resume/replacement are covered, but synthetic private MoveProgress/speed-snapshot mutation is not a separate fixture. Fresh 6,184 agreement and measured speed/RSS are required before adoption; controller tests alone prove neither.
