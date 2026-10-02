# S26c — public-history-closed growing-tree CFR

Feature `experiment-growing-belief` implies S26b `experiment-public-belief`, and remains
default OFF. Production planners and the battle engine are unchanged. Generic x86-64 is
supported; no AVX2/CUDA requirement was introduced.

## Algorithm and boundaries

`tree::builder::growing::search` keeps an explicit tree and a frontier of physical
histories. It admits the whole root first. Later selection walks mix a PUCT argmax with
the current average CFR policy, with probability 1/2 each, for both seats. Chance edges
are sampled at their exact retained probabilities. Q aggregates counterfactual action
values over every member of an information set, with the opponent's sign reversed.
The default prior is uniform. A replaceable `Prior` sees information/menu data, not a
sampled true state. Visit counts share the information key across hidden histories.

The sampled leaf identifies a PUBLIC history prefix. Admission then expands **every**
physical history with that prefix, including zero-current-reach histories, all worlds,
every legal joint action and every chance outcome. A player's own action memory remains
private; the second seat cannot inspect the first seat's hidden commitment. No physical
histories merge. No menu pruning or chance-mass truncation is used. Since a full public
history has a unique public parent prefix, new hidden members cannot arrive later from
a different admitted public parent. This closure avoids world-dependent depth and partial
information-set expansion.

The finite-tree CFR kernel remains S26b's signed alternating linear CFR. Growth changes
the game, so regrets and averages **restart** after every committed growth transaction.
Previously computed engine transitions remain stored and are not re-enumerated. This
reference favors explicit correctness over retained-regret efficiency; it clones the
candidate tree/frontier for rollback and solves the resulting surrogate afresh.

The method is inspired by [PokaiTrainer §4.3](https://arxiv.org/html/2608.29197v2#S4.SS3)
and its GT-CFR/Student of Games lineage. It is not a numerical reproduction: no trained
policy/value network, belief-dependent CFV leaf, cost-width exponent, action shortlist,
or safe subgame re-solving is supplied. Fixed leaf utilities still come from `Domain` /
the existing replaceable Evaluator. It should not be used to claim Pokai strength parity.

## Limits, rollback, and output semantics

`max_expansions` counts public-prefix admissions, including the root and mandatory switch
closure. `Limits.max_transitions` counts attempted transition invocations and charges
before calling the engine; rejected admissions do not refund work. `max_nodes` bounds
stored candidate tree nodes, and `max_decisions` bounds decision depth. `max_walks` also
counts selection walks that finish at existing terminal/horizon nodes.

Every pending mid-turn or forced replacement is expanded before committing a new
snapshot, even at the last requested turn. If a cap interrupts this closure, the whole
candidate is discarded. Later exhaustion returns the previous complete CFR snapshot;
if even the root is incomplete, the function returns an error with no strategy.
Malformed probabilities, menus, observations or evaluations are errors, not silent
fallbacks. Invocation and solver calls are indivisible: no strict wall-clock deadline or
peak-RSS guarantee is implied. Cumulative attempted and committed work are reported
separately, as are solver iterations across all transactions.

Unexpanded normal-turn frontiers have fixed heuristic values. Their current surrogate
may be solved exactly while the requested horizon remains incomplete. CLI therefore sets
`converged` only if both horizon completion and solver convergence hold, reports
`surrogate_converged` separately, and leaves `full_horizon_gap` null until full expansion.
`finite_game_gap` always describes the current supplied finite surrogate. Neither that gap
nor a complete finite horizon certifies the real battle beyond the leaf evaluation.

## CLI

Build `lab-belief` with `--features experiment-growing-belief`. Add to an S26b engine request:

```json
"growth": {"max_expansions":16,"max_walks":256,"exploration":1.0,"seed":1}
```

Omit `growth` to use the unchanged exhaustive path in the same binary. The engine still
requires `observation_model: "snapshot-information-v1"`. **A complete public battle-event
emitter has not been implemented.** Reconstructing events from final state is unsound:
existing engine enumeration merges some event histories before returning results. This
feature leaves the engine unchanged and does not claim full Showdown-log equivalence.

`mode: "observed"` accepts explicit transition tables for integration/oracle fixtures.
Each table state has `phase`, `value`, `public`, two `private` strings, and (if nonterminal)
two action menus plus a rectangular joint-transition array. Each cell contains
`[{"probability":0.5,"to":3}, ...]`. World seeds contain `id`, `weight`, `position`.
State IDs are never observations. Explicit different public transcripts remain different
histories even when state payloads/utility coincide. The caller must preserve and provide
the real event traces; this input format is not an engine log reconstruction facility.
`oracle_tree` in this fixture mode exports hidden histories for independent verification,
never for the playing agent's observation stream.

## Validation

Rust checks cover hidden-world closure, counterfactual Q sharing, zero-prior support,
exact work accounting, deterministic walks, root rejection, whole-transaction rollback,
node/depth/transition caps, switch closure, and full-growth/exhaustive agreement.
`growing_check.py` generates 24 different two-stage hidden-information games and compares
four snapshots each (full reference; growth budgets 1, 2, and complete) with independent
normal-form SciPy/HiGHS LP and pure-contingent-strategy best responses. It separately
checks the physical game tree after full growth, information-set counts, rollback, and
rejection of an incomplete root. The prior 49 extensive and 72 matrix LP checks remain.
No local timing comparison or speed/playing-strength claim is made.
