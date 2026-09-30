# Independent P8d / P8e / P8f experiment

The common baseline enables P8g hurt readers, P8c prepared turns, P9 leaf ending states, and P10 compact volatiles. Each candidate adds exactly one new runtime feature. Both timed arms use source `048e4748e8854dac5eb511a358cc2f8c38c9dc29`, derived from validated combined source `b22edb9dc425504da16a55594ef04178ab24e191` (successful Actions run `36665429800`). Production/default features remain unchanged.

| Candidate | Runtime feature | Scope |
|---|---|---|
| P8d | `experiment-replay-action-keys` | Reuse priority/speed keys within one restored stage input; retain original tie RNG; exclude initial priority draws, switches, resumed hits, sampling and factored paths. |
| P8e | `experiment-slot-diff` | Same occupied Slot and every other field identical: replace synthetic Switch reconstruction with existing scalar Set instructions. Exhaustive field guard and original fallback. |
| P8f | `experiment-stats-off-cost` | Snapshot LAB_ENGINE_STATS presence once per flat enumeration, use clocks only when enabled. Empty values enable stats; a mid-call environment change takes effect on the next call. Factored path unchanged. |

Accuracy runs four variants over the same frozen 6,184-case corpus (3,056 oracle, 2,808 turn, 320 search). Existing oracle, legality scope, hidden-state, rollback, full-State/hash, probability/order, and P8c/P9 activation gates remain. P8d/P8f require exact original streams. P8e preserves raw streams and separately canonicalizes only the turn outcome `instructions` field; all other fields and row order remain in its semantic digest. Its compact probe exempts only `fixture.records[].endings[].instructions`. Search and oracle comparisons are unchanged.

Each speed job runs complete release regressions in both arms and separate exact observer tests before timing. All timing observers are OFF; LAB_ENGINE_STATS is removed from timed/RSS child environments. Generic x86-64, one thread, coaching/sand depth2 beam2 cap2 Median, 10 alternating AB/BA pairs, excluded warmups, separate RSS. No local performance benchmark is used. A verified executable cache cannot replace fresh regressions in these new modes; dependency compilation caching remains allowed.

Before dispatch: individual OFF/ON and dense/compact checks passed, as did merged-source core 111 tests, new integration 11 tests, and existing prepared-turn 9 tests. Independent source and comparator reviews found no blocker. Local strict Clippy encountered an existing P10 `compact.rs` manual_is_multiple_of lint; that unrelated baseline source was not modified. Performance and broad-corpus acceptance remain pending the fresh workflow.
