# S26b: finite information-tree CFR and belief continuation

`experiment-public-belief` is default OFF, layered on the S26a Bayesian experiment.
The library adds a multi-turn, perfect-recall extensive-form solver, public/private belief
queries and an engine bridge that resolves mid-turn and KO replacements. `lab-belief`
exposes raw finite games and bounded engine scenarios. No production planner is switched.

## Information and CFR

Each physical history remains a separate tree node. A decision refers to an information
set whose members share one action menu and one regret table. The constructor rejects
cycles, shared history nodes, unreachable nodes, inconsistent menus/owners and forgotten
own information/action sequences. Different initial types can share our information set;
the informed opponent's types differ. Raw trees also support two-sided private information.

The domain compiler sequentializes simultaneous choices without disclosing the first
choice to the second player. Public observations are remembered by both; each player's
private observations and own actions are remembered only by that player. A suspended
opponent command stays hidden unless the observation contract reveals it. World identity,
queue contents, full State hashes and the transition branch index are not observation keys.

For each alternating player pass, policies remain frozen while accumulating regret over
ALL member histories. Regret increments use `t * opponent-and-chance reach * advantage`.
Only after accumulation are that player's policies updated. Average strategies use
`t * own realization reach`, once per information set. Weighting by member count or full
history probability is incorrect. Per-world perfect-information re-solving is not used.

Best responses also choose ONE action per information set. They aggregate continuation
values using opponent-and-chance reach before selecting an action. The reported upper
bound is our best-response value; the lower bound is the opponent's best-response value
(both from our perspective). `finite_game_gap = upper - lower` certifies the fully built
finite game, to numerical precision. A low gap does not certify omitted observations,
the real battle beyond the horizon or the leaf evaluator's quality.

## Beliefs, cursor and leaves

Current-strategy history reach induces the posterior; it is not a fixed average of worlds.
`Tree::public_beliefs` groups full PUBLIC observation histories and sums reach by world.
`Cursor::belief` also conditions on the owner's private observations and remembered own
actions. These can differ: a public signal may be ambiguous until our own hidden action
is taken into account. Both APIs retain horizon endpoints, world IDs, physical-history
weights and conditional world values. A zero-reach history has `posterior: None`, not an
invented uniform prior. The returned conditional values are diagnostics, not learned CFVs.

`Cursor::new(0, None, observation)` cannot accept a hidden world ID. After each transition,
`advance(own_action_id, observation)` identifies the next policy through `information`.
This retrieves the same continuation at indistinguishable histories. At a horizon endpoint
there is no further policy, but `belief` still supplies the posterior for another search.
Opponent cursors require their known type ID. New external observations must have exactly
the same semantics as the domain used to build the tree; an absent key is not guessed.

Leaves are fixed utilities from the replaceable existing Evaluator (terminal scores use
the existing WIN convention). CFR evaluates their counterfactual contributions throughout
the game. No belief-dependent neural leaf model or independently solved full-information
child value is substituted. These interfaces leave evaluation outside the regret kernel.

## Engine observation abstraction

The current engine merges some histories with equal state before exposing outcomes. It
does not export a complete public event transcript. Therefore the bridge requires an
explicit `snapshot-information-v1` model. This is an information abstraction, NOT a claim
of Showdown-log equivalence or conservative strength against a fully observing opponent.

The observer allows public decision phase, turn/result, active species (Illusion is
rejected), HP percentage buckets, status, boosts, switch requests and active field/side
effects. It does not expose effect timers. Champions HP buckets use floor with minimum
one for living Pokémon; boundary color distinctions are coarsened. Own HP, current
item/ability and move/PP request data are private observations. Hidden bench identity,
random status durations, substitute HP, ability ordering, queued actions and raw last-move
fields are omitted. Executed move messages, move order and within-segment reveals are
not reconstructed from state deltas. `ObservedDomain` can be replaced by a future engine
that preserves event traces. New mechanics require a disclosure-contract review.

The existing S26a `Knowledge` check still conservatively validates initial worlds: our
entire side and undeclared fields agree, optional hidden non-HP stats may differ, and
explicitly unrevealed reserves may differ only at opening roots. Team-sheet/history
consistency is the caller's responsibility. Ambiguous scenario setup is rejected. Roots
start at a normal turn; an arbitrary suspended root without past action memory is rejected.
Within the built tree, suspended turns and forced replacements ARE handled.

Full damage rolls and all legal actions are enumerated. Turn horizon decrements only for
normal turns; switches are resolved even after the last turn's budget is consumed. A node,
transition or decision-depth limit aborts the whole build. No incomplete game is returned
as solved. One enumeration is indivisible, so limits are not strict wall-time/RSS limits.

## CLI examples

From `engine/`:

```
cargo run --locked -p lab-search --features experiment-public-belief --bin lab-belief -- request.json
```

A two-turn engine request (paths relative to the request file):

```json
{"mode":"engine","observation_model":"snapshot-information-v1","side":"p1",
 "worlds":[{"id":"candidate-a","weight":3,"scenario":"candidate-a.json"},
           {"id":"candidate-b","weight":1,"scenario":"candidate-b.json"}],
 "knowledge":{"hidden_stats":true},
 "limits":{"turns":2,"max_nodes":50000,"max_transitions":20000,"max_decisions":32},
 "solver":{"iterations":20000,"tolerance":0.01,"check_every":32}}
```

Raw finite-game request:

```json
{"mode":"tree","root":0,"include_keys":true,"nodes":[
 {"type":"chance","edges":[{"probability":0.5,"child":1},{"probability":0.5,"child":2}]},
 {"type":"decision","player":0,"information":"unseen-type","actions":["a","b"],"children":[3,4]},
 {"type":"decision","player":0,"information":"unseen-type","actions":["a","b"],"children":[5,6]},
 {"type":"terminal","value":1},{"type":"terminal","value":-1},
 {"type":"terminal","value":-1},{"type":"terminal","value":1}]}
```

This game's value is zero: the same action distribution must serve both hidden types.
Output includes all information policies, finite-game bounds, convergence status and, for
engine trees, public beliefs. `include_keys` adds memory keys and history weights; these
are developer diagnostics, not an observation stream to feed the playing agent.

## Validation / next boundary

Tests cover Kuhn poker's known value -1/18, a hidden versus revealed queued-action pivot,
public signaling posteriors, own-action-dependent posterior at the horizon, perfect
recall rejection, continuation cursors, input restoration, resource limits, two actual
engine turns, U-turn/Eject Button, KO replacements and actual differing Suspensions with
the same observation. CI independently expands 48 generated extensive games plus Kuhn
into pure contingent strategies, solves a normal-form LP and recomputes each reported
best-response bound. The preceding 72 Bayesian matrix LP checks run unchanged.

Remaining work is distinct: event-preserving battle observations; scalable selective
GT-CFR/PUCT expansion; re-solving safety; learned belief-dependent CFVs; speed/strength
evaluation. A fully enumerated bounded reference implementation does not establish those.

Primary references:
[PokaiTrainer §4](https://arxiv.org/html/2608.29197v2#S4),
[Brown & Sandholm 2019](https://ojs.aaai.org/index.php/AAAI/article/view/4007),
[Showdown public health](https://github.com/smogon/pokemon-showdown/blob/master/sim/pokemon.ts).
