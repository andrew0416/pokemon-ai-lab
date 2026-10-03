# Borrowed public-boundary lookup and historical controls

Parent S25: `2bb9cf595f675d269406bbe783e88154ed517cb4`.
S24 reference: `d90c80b4001e43807d404454280f1511329ef952`.
Experimental `pipeline.borrowed_boundary` defaults to false; production unchanged.

## Representation contract

The old growing builder clones `Vec<String>` before every boundary lookup, including
hits. The new path borrows `[String]` with exactly the Hash/Eq behavior of the owned
vector. It clones only on a miss. The same derived Debug format creates the exported
public key. Both paths share private-key interning and boundary insertion. Neither
information ID assignment order, public-frontier lexicographic order, observations,
menus, callbacks, evaluator values, chance mass, solver or certificate changes.
This is not a hash-only identity, observer cache or altered disclosure policy.

The flag is a growing-request storage option; fixed-tree requests reject it. The
private Growing layout and helper boundaries can affect the OFF path in an experiment
build. Compare against pinned predecessors instead of assuming that overhead is zero.

## Correctness and ordinary comparisons

Inherited Cursor and trace tests are retained. New callback trace comparisons cover
the boundary flag with encoded history both OFF and ON, switch closure, budgets,
fork/in-place storage and the other pipeline flags. Original compiled tree metadata,
solutions, work and error behavior must match. The expanded oracle uses a fresh seed,
escaped future semantic labels and both history modes; it compares entire OFF/ON JSON
and independent exhaustive BR/LP, with capped nonconvergence separately recorded.

Eight settings from six bounded engine fixtures. Three same-binary pairs, each with
an identical-call A/A, use 9 ABBA/BAAB blocks: option alone, on top of encoded history,
and on top of the previous five options. Total 48 comparisons. The 56 profile
configurations are separately checked across clean, phase and snapshot binaries;
phase timings (5 captures each) and allocator counts are diagnostic, not speed gains.

## Pinned version controls

S24 and S25 are checked out at exact SHAs on the temporary runner. Only their benchmark
CLI is replaced by the exact S26 CLI, after verifying the original Git blob hash and
that the only working-tree diff is this file. Libraries/engine are untouched. Patch,
source SHA and binary SHA256 are recorded. All three use Rust 1.98.1 and generic x86-64.

The common stdin transport keeps input loading and pool creation outside each sample.
Three persistent processes receive commands serially, with up to four CPUs shared;
workers are never requested to compute simultaneously. One warm computation per
binding precedes 9 rotated ABBA/BAAB blocks, one compute per sample. Every sample's full
result/work witness must match the preflight. JSON, stdin wait and returned-result
destruction are outside compute. Input/pool ownership and code generation still differ
between versions; the comparison intentionally includes library layout changes.

Six comparisons per setting (48 total): S24 A/A, S24→S25 all options OFF, S25 A/A,
S25→S26 OFF, S25 encoded/full-five A/A, and S25→S26 full-five (boundary OFF).
This guards both the earlier history change and this candidate's OFF wrapper cost.
Intervals are exploratory within-run estimates, not a proof of zero overhead or a
statistical multiplicity correction. The local worker smoke test uses only current
binaries with timing disabled and is not historical-version performance evidence.

## Sand RSS repetition

Use 12 rotated blocks of five fresh processes per mode: S25 all-four, identical A/A,
S25 all-five, S26 all-five, S26 all-six. Repeat separately for clean `--check` and
allocator-instrumented `--memory`: 120 processes. `/usr/bin/time` measures each child's
whole peak RSS, including input/pool/loading/witness; modes must not be pooled.
Each result/work witness equals the corresponding clean reference. Instrumented
requested heap and whole-process RSS have different scopes. The repeats assess the
previous 5.58% sand RSS observation; they do not diagnose fragmentation or leaks alone.

Non-AVX2 Nehalem emulation verifies CPUID and exact native/emulated results for two
engine cases × seven configurations and the legacy scalar solver. This is not real
old-hardware timing. All experiments are speed/correctness only; no win rate evaluated.
