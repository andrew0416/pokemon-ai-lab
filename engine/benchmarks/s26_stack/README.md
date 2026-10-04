# Direct comparison of the retained five-option stack

Compare S24 `d90c80b4001e43807d404454280f1511329ef952`, all five pipeline
options OFF, directly with the unchanged S25 library
`2bb9cf595f675d269406bbe783e88154ed517cb4`, five ON:
`frontier_index`, `owned_compiler`, `incremental_sequence`, `compressed_checks`,
`encoded_history`. The rejected borrowed-boundary option is absent from this library.
All defaults remain OFF. This does not measure every historical engine optimization
against the original project; it measures these five cumulative pipeline options.

## Inputs and preregistered comparisons

Eight inherited settings plus two unmodified heldout oracle fixtures: Unseen Fist
and protection, and Mega Sol/Synthesis. Same worlds, evaluator, limits, LCFR/periodic
sequence solver, flat enumeration and public/private observation abstraction.
The Metronome/Protect fixture was excluded during local input validation because
random setup damage gives multiple roots. No timings informed that selection.

Each eligible setting has 9 rotated ABBA/BAAB blocks for parent A/A, selected A/A and
the direct parent→selected comparison. The previously flagged narrow depth3/cadence1
setting also has parent→S25 OFF and S25 OFF A/A: 32 comparisons when all 10 settings
finish the finite game and converge. Incomplete or capped settings are explicitly
censored rather than silently removed or called gains. The 95% bootstrap intervals
are exploratory and not corrected for multiple comparisons. A/A is a descriptive
noise screen. Report the direct ratio, never products of earlier ratios.

## Correctness and measurement boundary

The libraries are unchanged. Existing S24/S25 independent BR/LP, rollback, callback,
snapshot and scalar/non-AVX2 evidence is retained; those suites are not claimed as
newly rerun by this benchmark. The only Rust edit is the common benchmark CLI.
Two separate Cargo target directories prevent historical executable reuse; source
pins, sole reference-file diff and executable SHA256 are saved and checked.

Worker requests run serially. Input and pool loading, stdout/JSON, full-result
witnesses and result destruction are outside compute. A worker destroys the result
before acknowledging its response, avoiding overlap with another worker's next solve.

Every sample preserves its route's full Tree/Solution/diagnostic witness. Across
OFF and selected routes, an additional streaming FNV-1a-128 witness and byte count
cover the entire Tree, complete Solution and search diagnostics, excluding only the
`paper` and `pipeline` optimization counters. Those counters remain in the full
witness and raw output; different counts are expected for these optimizations.
Search walks, expansions, transition calls, solves and CFR iterations are retained
in the cross-route check, as are values, policies and original-tree certificates.
The witness is a regression check, not a cryptographic proof or an independent oracle.
Evidence-file hashes are SHA256.

Local `--mode check` uses only the current binary and no timer; it checks transport
and exact OFF/selected outcomes. Historical cross-version equality is checked again
on the runner before timing. Generic x86-64 is mandatory; no AVX2 requirement.

## Bottleneck selection, not another micro-optimization

Separate phase builds record five samples each for OFF and the retained stack.
Their overhead makes them diagnostics, never the source of headline speed ratios.
Select a large, source-mapped cost with a plausible >=5% whole-compute reduction.
Show the required reduction in that component and the semantic preservation contract.
Small clone/allocator candidates stay deferred. No new optimization implementation,
production adoption, win-rate or strength evaluation is part of this experiment.
