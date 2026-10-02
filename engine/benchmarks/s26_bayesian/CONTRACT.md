# S26a: one-sided Bayesian matrix game + linear CFR

Status: isolated experiment, `experiment-bayesian-cfr` is **default OFF**. This is an
additive library and `lab-bayes` CLI. Production planners and the S25 selective search
do not automatically call it. Generic x86-64; no AVX2 or CUDA requirement.

## Information and algorithm

- The row player knows its own configuration but not the opponent's world. The opponent
  is assumed to know both configurations, including its private world. This is one-sided
  private information, not a model of both sides' actual private knowledge.
- There is ONE row policy `p`, a probability distribution `b` over worlds, and ONE column
  policy `q[w]` per world. Column menus can differ. Row action identity must agree.
- The objective is `max_p min_{q[w]} sum_w b[w] p^T A[w] q[w]`.
- Signed cumulative regrets receive linearly weighted increments. Our regret update
  precedes each informed opponent update; the latter uses the new row policy. Each
  player's post-update policy enters the running average with weight `t`. This is
  alternating linear CFR, not merely RM+ with a new label.
- Payoffs share a positive numeric scale; regret weights use `t / max_iterations` to
  bound arithmetic. Dividing each positive-prior opponent regret table by its constant
  chance reach leaves regret matching unchanged. Zero-prior worlds remain indexed with
  uniform column policies. They are never silently removed from the input.
- Fixed priors and payoffs are immutable during a solve. Conditioning returns a new
  game and starts fresh regrets. Caller-provided likelihoods must account for observed
  action behavior where needed, not just whether an action is legal.

`Game::assess` independently evaluates average policies:

```
lower = sum_w b[w] min_j sum_i p[i] A[w][i,j]
upper = max_i sum_w b[w] sum_j q[w][j] A[w][i,j]
gap   = upper - lower
```

The lower/upper bounds enclose the equilibrium value of the **supplied fixed matrices**,
up to floating-point error. The gap is the sum of both deviation gains. Exhaustion may
return `converged: false`; `iterations` is not a convergence guarantee. No claim about
full battle exploitability, win rate, or speed follows from this certificate.

## Library boundaries

`bayesian::{Game, World, Config, solve}` has no JSON/scenario requirement. The feature
also enables the existing engine-domain adapter feature but invokes no selective search.
`Game::new` rejects invalid IDs, shapes, nonfinite data and impossible beliefs. Positive
belief underflow is an error. `conditioned` matches likelihoods by world ID and rejects
missing/duplicate/unknown IDs and observations contradicting every positive-prior world.

`bayesian::engine::one_turn` accepts borrowed `EngineWorld`s and a replaceable `Evaluator`.
It enumerates every legal joint action with `Pruning::All` and `RollMode::Full`, then
evaluates the next turn or terminal state. It checks chance mass and uses the existing
immutable-input transition adapter. Errors reject the complete game; no partial or
renormalized matrix is published. `max_cells` caps enumeration CALLS, not memory or the
duration of one engine call.

The conservative `Knowledge` boundary keeps our entire side and all undeclared fields
identical. Optional hidden stats permit only opponent non-HP stats and stat-point
allocations to differ; exact current/max HP must still match. Optional unrevealed
reserves permit different opponent party members at explicitly listed inactive slots,
only at turn 1. The caller must validate those hypotheses against the known team sheet
and observation history; this interface does not infer either. Conservative equality
can reject indistinguishable positions that a later public-state abstraction could merge.

Root replacements and suspended moves are unsupported. If ANY enumerated outcome needs
a KO replacement or mid-turn switch, the adapter errors rather than solve that decision
separately with knowledge of the hidden world. This prevents strategy fusion at the
current boundary. The raw matrix API remains useful for other correctly constructed games.

## CLI

From `engine/`:

```
cargo run --locked -p lab-search --features experiment-bayesian-cfr --bin lab-bayes -- benchmarks/s26_bayesian/matrix-example.json
```

The input is a strict JSON object. Optional `solver` fields: `iterations`, `tolerance`,
`check_every`. Output contains one `ours` policy, per-world `theirs` policies, posterior
weights, conditional world values, lower/upper bounds and convergence status.

Matrix input (flat row-major payoffs):

```json
{"mode":"matrix","rows":["attack","protect"],"worlds":[
  {"id":"fast","weight":3,"columns":["attack","switch"],"payoffs":[3,0,0,1]},
  {"id":"slow","weight":1,"columns":["attack"],"payoffs":[-1,2]}
]}
```

Engine input:

```json
{"mode":"engine-one-turn","side":"p1","evaluator":"heuristic","max_cells":100000,
 "knowledge":{"hidden_stats":true,"unrevealed_reserves":[]},
 "worlds":[{"id":"candidate-a","weight":1,"scenario":"candidate-a.json"},
           {"id":"candidate-b","weight":1,"scenario":"candidate-b.json"}]}
```

Scenario paths resolve against the request's folder. Each must produce exactly one
initial position; ambiguous setup branches cannot be silently selected. Hidden-reserve
mode additionally rejects setup turns. `likelihoods`, if supplied, is an array of
`{"world":"candidate-a","probability":0.8}` covering every world exactly once.
Evaluator names follow the existing `load_evaluator` API; evaluation is independent of CFR.

## Validation and future integration

Rust tests include strategy-fusion and average-matrix counterexamples, analytic games,
24 generated games checked against expanded normal form with the independent existing
RM+ solver, posterior/invalid-input tests, Full engine enumeration in both orientations,
different opponent menus and rejection/input-restoration checks. CI separately compares
72 generated games against a SciPy/HiGHS linear program and recomputes every certificate.
OFF tests, no-default-feature library checks and clippy protect the additive boundary.

Not implemented here: a public observation-history representation, posterior updates
induced by policies INSIDE a search tree, counterfactual leaf values conditional on those
beliefs, belief-aware replacement/pivot decisions, GT-CFR expansion, learned policy/value
models or automatic 4-of-6 hypothesis generation. Calling S25's perfect-information
continuation search per world would not supply these missing contracts. A follow-up must
group observationally indistinguishable histories before offering a multi-turn API.

Primary reference: [PokaiTrainer, v2, sections 4.1–4.2](https://arxiv.org/html/2608.29197v2#S4).
This implementation covers its fixed Bayesian decision game, not the complete PBS search.
