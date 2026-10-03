# S26g growth pipeline experiment

All four options default OFF behind `experiment-growth-pipeline`.
The parent is S21/S26f `b10b08477f196ce0b9b361fa7878e311fdf1d4c8`.
Original production engine and old experiment paths are retained.

1. `frontier_index`: temporary terminal→public-group index and allocation-free chance sampling, exact RNG/prior/visit semantics.
2. `owned_compiler`: consume the compiled snapshot only AFTER the entire selected batch is admitted. Reuse child vectors and labels; full recall/reachability validation still runs. Any compile/solve failure returns an error. Budget failure still returns the last solved tree.
3. `incremental_sequence`: requires owned compiler and sequence=true. Stable own-action IDs, remove replaced heuristic leaf records, traverse only their new subtrees, re-sum affected payoff coefficients in original DFS order. A scale change recomputes coefficients from unscaled records. Layout/order/entry vectors are still rebuilt; this is not a fully persistent solver and regrets still restart. Positive chance underflow falls back to the scalar path. One-turn horizons skip both reuse caches because there is no second snapshot.
4. `compressed_checks`: requires sequence=true and compact=false. Sequence-form best responses preserve information constraints. They only reject non-converged internal check candidates. Near-threshold/unsafe candidates and every final iteration use the original Tree assessment; only that original certificate is returned.

Stage 1/2 require exact policy, iteration, raw tree and work parity. Stage 3 may change floating-point accumulation order (stable sequence IDs) and therefore selection/iteration details; tests independently certify the actual exported game. Stage 4 retains the original final certificate but may theoretically alter stopping iteration near roundoff.

Speed: one generic x86-64 release binary, runtime flags, common LCFR sequence baseline, periodic32, cadence4, same evaluator/seeds/budgets and pools. Six engine fixtures × five comparisons, plus six OFF controls. Stage 3 is compared directly to owned-compiler-only; the other isolated changes and all-four compare to S21. Nine interleaved ABBA/BAAB blocks. No local timing, AVX2 requirement or win-rate claim. Capped unequal-gap comparisons are censored. Paired intervals describe one runner session only.

Correctness: existing core/search/owned tests and inherited tree/growing/cadence/paper LP checks, plus new-seed exhaustive pure-plan/LP certificates for fixed and growing games, partial budgets, invalid options and atomic rollback.
