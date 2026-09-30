# P15 R1 versus R1 plus prepared-leaf validation sharing

Same immutable source, generic x86-64 and one thread. R1 contains the existing
four optimizations plus P8d and P13. E/F/P11/P12/P14 stay off. Candidate adds only
`lab-search/experiment-prepared-leaf`, forwarding to its core implementation.
Runtime and observer features are independent, default off. All older feature
contracts, frozen workloads, corpus and Rust probes are retained unchanged.

Fresh 6,184 oracle/turn/search cases require exact raw output, with no new legality
or instruction exception. P15's diagnostic change is handled by a versioned,
candidate-only eight-deep-control contract that binds the actual frozen case IDs
and ordering, exact output SHA/bytes, unchanged four P9 work counters and positive
decreases in real validators. R1 retains the old equal-validator precedence
contract. The frozen exact-depth-two nonleaf probe still executes unchanged.

Full release regressions and six bounded debug proofs run before measurement:
dense/compact with observers on each runtime arm, plus compact without observers
on each arm. Each proof writes the same complete 54-case JSONL; all six streams
must match byte for byte. Candidate private trace tests and existing P9
no-partial-batch tests retain the error and evaluation order contracts. The
two actual workload observer builds simultaneously verify prepared-leaf requests,
the three validator counters, all P9 work and R1 P13 borrowed lookup counters.
Observer results and source-owner local proofs are not speed measurements. Timing
uses the untouched harness, ten alternating AB/BA pairs for each narrow case and
separate two-pair RSS collection, after every required proof passes.

Cache identities bind every injected probe. Dependency compilation may be reused;
old correctness results never substitute for this run's checks. No source/runtime
adoption follows from preparation alone.
