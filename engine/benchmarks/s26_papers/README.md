# S26f: paper solvers, strategy reuse, sequence contraction

Experimental descendants of frozen S19 `7f4ca4397e098d7877ba4f25b79e1164d21a080a`.
Enable `experiment-paper-solvers`; the default feature list and original solver
remain unchanged. One portable x86-64 binary selects treatments at runtime.
No AVX2, CUDA, new dependency or production adoption is required.

## Independent options

The CLI accepts `solver.paper`. Omitting it invokes the original implementation.

```json
{"rule":"sapcfr+","checks":"geometric","compact":false,"sequence":true,"reuse_policy":false,"warm_iterations":0}
```

- `rule`: `lcfr`, `cfr`, `cfr-simultaneous`, `dcfr`, `pcfr+`, `sapcfr+`,
  `hs-dcfr-15`, `hs-dcfr-30`, `hs-pcfr-15`, `hs-pcfr-30`.
- `checks`: original `periodic` or powers of two (`geometric`), always including
  the iteration cap. Checks use the original tree's constrained best response.
- `compact`: one information set per player and exactly one choice by each on
  every history; sum chance outcomes into a normal-form matrix. Other games fall
  through to the next selected kernel or the original history traversal.
- `sequence`: general perfect-recall sparse sequence-form payoff contraction.
  Each own action is a sequence; the validated own recall determines its parent.
  Terminal histories contribute chance-weighted, scaled payoffs to pairs of last
  sequences. Multiply by the opponent's realization plan, then propagate own
  continuation values backwards to compute counterfactual action regrets.
  Distinct information sets/observations are retained. At most 1,048,576 sequence
  slots and payoff entries; otherwise use the original kernel. Dangerous tiny
  reach products also use original history arithmetic for that player update.
- `reuse_policy`: map the previous average by player, information identity,
  action labels and semantic own recall. Unmatched information is uniform.
  Reassess on the **new tree**; accept only if its original-tree gap passes.
  Otherwise restart, unless a valid warm initialization was explicitly selected.
- `warm_iterations`: 0 (off), or explicit virtual history T up to 1,000,000.
  Requires `cfr-simultaneous`. Rebuild substitute counterfactual values in the
  new game's scale; never copy old regrets or averages. Enforce the 2016 paper's
  squared positive-regret bounds and nonpositive sum of substitute root values.
  Start at lambda=1 and bisect 16 times toward the root-sum boundary, keeping
  only feasible initializations. Fall back cold if none is feasible.
  Seed averaging is T times **own realization reach**, not a local probability
  mixture. Virtual iterations are reported separately from executed iterations.

Matrix/sequence contraction changes floating-point summation order. It promises
the same mathematical game and independently checked certificate, not bitwise
policy identity. Only the plain LCFR/periodic/full-history control must exactly
match the reference arithmetic and policy. Partial growing searches may select
different frontiers; they certify their current surrogate only. Atomic cadence
admission, work charging and unsolved-batch rollback are unchanged.

## Rule details and primary sources

LCFR retains the reference per-history linear weighting and alternating updates.
DCFR discounts positive/negative cumulative regrets with exponents 1.5/0 and
quadratic averaging. PCFR+ projects regrets after aggregating all members of an
information set, then predicts the next instantaneous regret with coefficient 1.
SAPCFR+ uses coefficient 1/3. Predictive variants retain quadratic averaging.
HS uses configured iteration horizon N: alpha=1+3t/N, beta=-1-2t/N and
gamma=15-5t/N or 30-5t/N. The experimental alternating HS path does not claim the
simultaneous-update theorem verbatim. Formal warm initialization is isolated to
plain simultaneous CFR to avoid transferring its conditions to these variants.

- [Brown & Sandholm, DCFR, 2019](https://arxiv.org/abs/1809.04040)
- [Farina, Kroer & Sandholm, PCFR+, 2021](https://ojs.aaai.org/index.php/AAAI/article/view/16676)
- [SAPCFR+, 2026](https://ojs.aaai.org/index.php/AAAI/article/view/38766)
- [HS schedules, 2026](https://ojs.aaai.org/index.php/AAAI/article/view/38784)
- [Warm starting, corrected 2016 paper](https://www.cs.cmu.edu/~sandholm/warmStart.aaai16.withAppendixAndTypoFix.pdf)
- [Koller, Megiddo & von Stengel, sequence form, 1996](https://ai.stanford.edu/~koller/Papers/Koller%2Bal%3AGEB96.pdf)

Warm T=8/32 are explicit experimental choices, not the paper's empirical T-fitting
procedure. No asymptotic rate, speedup or playing-strength result is presumed.

## Verification and timing contract

`check.py` uses a new seed for hidden-world games, chance-weighted random matrices
in both player orders, Kuhn poker, observed continuations, partial budgets and
pending-batch rollback. Python enumerates pure contingent plans independently.
It verifies profile value and both best responses. CI additionally requires a
SciPy/HiGHS LP value inside the certified interval. Counts distinguish games from
multiple algorithm/budget checks on each game. Local runs are correctness only.

`run.py` refuses timing outside GitHub Actions. The fixed phase excludes engine
construction, but includes kernel compilation, allocation, CFR and original-tree
certification. The growing phase includes construction and all intermediate
solves. Warm variants compare against simultaneous CFR cold, keeping the rule
constant. Other growing variants compare with original S19 cadence4. Both sides
share the same pool: pivot serial, other fixture families explicit four threads.
These are measured-fixture settings, not an automatic production classifier.

Each pair must complete the same entire finite game at the same gap threshold.
Nonconverged caps are censored rather than reported as faster. Seven alternating
ABBA/BAAB blocks rotate variant order. Paired bootstrap intervals describe this
runner's blocks, not across-machine uncertainty. Reports include iteration and
assessment counts, compression activation, reuse acceptance and warm attempts.
These experiments measure speed and certificate validity, never win rate.
