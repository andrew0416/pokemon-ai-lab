# P7d factored BeforeFirstHit pilot

This isolated controller inherits 6508953962bc46ed7c6a7780a0e859b3444231ff without changing inherited files. The candidate source directly inherits frozen P1e validation source 56376838ffea91169321895f8e81abec2137dd46. The runtime feature experiment-factored-first-hit remains default OFF.

## Pipeline and release gates

1. Verify clean immutable reference and candidate checkouts, exact candidate delta and source review/logic receipt pins. Verify the original B17 corpus and eight original selection-plan byte strings. Neither benchmark nor joint exporter is modified.
2. Restore and save dependency compilation only (workspace crate caching remains disabled). Reject cached lab workspace fingerprints. Compile three primary arms separately: reference563 + core6 + P1e ON; new source + same features (OFF); identical new source + P7d (ON). A fourth core-test-only build enables P7d and disables P1e.
3. Run 85 fresh named tests: immutable exporter six in each of three arms; immutable benchmark seven in OFF and ON; nine scoped P7d tests in both P1e modes; 35 existing core/scenario regressions under ON. Actual compiler JSON fingerprints, complete logs, executable hashes and argv are recorded and verified before execution.
4. Export all eight frozen decisions from reference/OFF/ON once. All descriptions must equal the original B17 plan bytes. Compare complete dictionaries (full non-HP State plus Suspension), every joint 12-party HP tuple, per-key raw probability within 1e-12, raw mass and normalized TV within 1e-9. Reference563 versus feature-OFF additionally requires byte-identical dictionary.json, joint.bin and selection.json payloads; manifest timings are excluded. Partial execution, missing records, mismatches or invalid outputs fail the accuracy gate. Each arm is attempted independently within the phase budget.
5. Only after all eight agree, run unchanged lab-distribution-bench on five frozen cases. One OFF/ON warmup per case is retained but excluded. Three ABBA blocks follow, in fixed case/block order. Each block ratio is sum(ON Full API kernel_ns) / sum(OFF Full API kernel_ns); report all three ratios and their median. Sampler timings, metric preparation, export time and whole-child time never enter this ratio.

## Scope and resources

Case selection was fixed from prior source563 run36870979964 and original B17 plans before any P7d measurements: 0000/0001 controls; 0099 Hyper Voice; 0110 Heat Wave; 0129 suspension; 0133 Earthquake; 0279 Rock Slide; 0429 expensive spread/fallback mixture. Timed cases are 0099/0110/0129/0279/0429. Equal raw Speed stats in fixture metadata do not prove a live speed tie or stage activation. The separate named core tests prove activation, prefix-entry reduction, fallback exclusions, errors, rollback/TLS, ordering and real pause/resume.

Every measured child is pinned to one CPU, generic x86-64, observer OFF, RAYON_NUM_THREADS=1, 300 seconds, 6 GiB RSS. RSS is polled every 10ms with terminal wait4 high-water accounting; transient overshoot is possible. Each accuracy/timing phase has a 900-second admission budget: a child starts only if its entire 300-second cap fits. Post-child validation/artifact bookkeeping can extend beyond this deadline. The job has a 35-minute hard limit and always uploads available evidence.

Existing regressions exercise absent-user/error cleanup, P1e lazy-frontier replay and rollback, established HP/heavy/reduced-roll factored fixtures, called-move completion tails, multi-hit, Emergency Exit and spread Eject Button ordering. Only three existing integration binaries are added, preserving consolidated test targets.

Operation counters occur only in cfg(test) named tests and their logs. The release executables have no observer feature or LAB_* environment. Counts are evidence of changed work, not a speed estimate. No sampler-state equality is inferred from scalar benchmark JSON: its derived metrics and counts are ancillary; the joint exporter is the independent full-state gate.

This is an eight-case pilot, not an all500 run, global equivalence proof or adoption decision. Reference563 itself contains P1e; the missing original-a4 opening0429 oracle is not repaired. No old-VM timing is divided by this run. Incomplete rows never produce a timing ratio, and failed rows cannot reuse a stale numeric result.

## Binding and publication

ci/bind.py reads the frozen source receipt, source review and 20-test local logic receipts, verifies source bytes and named logs, and replaces only the explicit source-binding/workflow placeholders. It does not invoke Git, Cargo, network or an engine. Root reviews and privately freezes the additive controller; only root publishes the exact source and controller branches. This workflow has one exact push trigger, separate concurrency, read-only repository permission, no manual dispatch, and uploads raw evidence even after a gate fails.
