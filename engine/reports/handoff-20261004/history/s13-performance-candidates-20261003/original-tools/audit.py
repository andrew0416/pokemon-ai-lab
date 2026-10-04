"""Read-only census of the frozen S12 source and existing GitHub artifacts.

No engine execution, benchmark, build, network call, or source edit is performed.
Outputs analysis evidence separately from the frozen experiment.
"""
from pathlib import Path
from hashlib import sha256
import json

HERE = Path(__file__).resolve().parent
BASE = HERE.parent / "s12-observation-writer-20261003"
SOURCE = BASE / "source"
PAIRED = BASE / "publication/run-37034440983/s26c-writer-37034440983-1/paired"


def digest(path):
    return sha256(path.read_bytes()).hexdigest()


def site(relative, needle):
    path = SOURCE / relative
    lines = path.read_text(encoding="utf-8").splitlines()
    matches = [i + 1 for i, line in enumerate(lines) if needle in line]
    assert matches, (relative, needle)
    return dict(path=relative, sha256=digest(path), lines=matches, text=needle)


sites = {
    "instruction_materialization": site("engine/core/src/turn/mod.rs", "let instructions = diff::instructions(start, end);"),
    "child_reconstruction": site("engine/search/src/budgeted/engine.rs", "child.apply(&outcome.instructions);"),
    "child_clone": site("engine/search/src/budgeted/engine.rs", "let mut child = state.clone();"),
    "existing_p9": site("engine/core/src/turn/final_states.rs", "pub fn try_enumerate_turn_final_states"),
    "p9_scope": site("engine/search/src/solve/leaf_endings.rs", "!matches!(next, Next::Depth(0))"),
    "history_clone": site("engine/search/src/bayesian/tree/builder/growing.rs", "let mut memory = h.memory.clone();"),
    "history_string": site("engine/search/src/bayesian/tree/builder/growing.rs", "format!(\"0:{:?}\", h.memory[0])"),
    "frontier_scan": site("engine/search/src/bayesian/tree/builder/growing/reuse.rs", ".find(|(_, g)| g.iter().any(|h| h.node == selected))"),
    "full_compile": site("engine/search/src/bayesian/tree/builder/growing/reuse.rs", "let next = compile(candidate, options.compiler)?;"),
    "fresh_cfr": site("engine/search/src/bayesian/tree/workspace/cached.rs", "let mut policy = tree.uniform();"),
    "assess_value": site("engine/search/src/bayesian/tree.rs", "let value = self.values(policy)[self.root] * self.scale;"),
    "assess_reach": site("engine/search/src/bayesian/tree.rs", "let cf = self.reach(policy, Some(player))?;"),
    "scores_value": site("engine/search/src/bayesian/tree/builder/growing.rs", "let values = t.values(policy);"),
    "scores_reach": site("engine/search/src/bayesian/tree/builder/growing.rs", "let cf = [t.reach(policy, Some(0))?, t.reach(policy, Some(1))?];"),
    "post_solve_scores": site("engine/search/src/bayesian/tree/builder/growing/reuse.rs", "q = scores(&next, &solution.policy)?;"),
    "serial_transitions": site("engine/search/src/bayesian/tree/builder/growing.rs", "let children = self.b.domain.transitions(p, [row, col])?;"),
    "own_reach_product": site("engine/search/src/bayesian/tree/workspace/cached.rs", "for &(j, a) in &info.own_sequence"),
    "cursor_key_repeated": site("engine/search/src/bayesian/tree/builder.rs", ".position(|i| i.key == self.key())"),
}

census = []
for path in sorted(PAIRED.glob("*-same-work-seed1-raw.jsonl")):
    rows = [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines()]
    ref = next(row["diagnostics"] for row in rows if row["type"] == "baseline")
    candidate = next(row["diagnostics"] for row in rows
                     if row["type"] == "validation" and row["variant"]["kind"] == "all-writer")
    assert ref == candidate, path
    growth = ref["growth"]
    assert ref["horizon_complete"] and growth["frontier_groups"] == 0
    assert growth["total_cfr_iterations"] >= ref["iterations"]
    row = {key: ref[key] for key in ("nodes", "information_sets", "transitions", "chance_outcomes", "leaves", "iterations", "gap")}
    row.update(case=path.name.removesuffix("-same-work-seed1-raw.jsonl"),
               growth=growth,
               intermediate_cfr_iterations=growth["total_cfr_iterations"] - ref["iterations"],
               artifact=str(path.relative_to(BASE)), artifact_sha256=digest(path),
               reference_candidate_diagnostics_equal=True)
    census.append(row)
assert len(census) == 6

candidates = [
    dict(id="S26e-owned-transitions", priority=1, status="proposal-unimplemented",
         title="Move engine ending states directly into the CFR adapter",
         scope="same-result-storage",
         basis=["instruction_materialization", "child_reconstruction", "child_clone", "existing_p9", "p9_scope"],
         change="Extend the P9 mechanism with an owning ending-state batch. Initially dispatch only eligible flat Turn transitions; keep replacement, resume and factored fallback. Retain the immutable caller/input-restoration check.",
         target_counts="One diff/instruction allocation and one parent-clone/apply reconstruction per chance outcome in the old adapter.",
         caveat="P9 currently exposes a borrowed visitor and is wired to the legacy solver's depth-zero path. An owning API and the new adapter integration are not implemented. Do not claim all outcomes are eligible or that elapsed time is proportional to their count.",
         verification="Exact states, probabilities and order, pending continuation, all error paths, caller restoration, and full tree/policy/work parity. Factored/unsupported phase fallback must be exercised."),
    dict(id="S26e-interned-observation-history", priority=2, status="proposal-unimplemented",
         title="Intern observation/action history in the builder, before tree compilation",
         scope="same-result-storage",
         basis=["history_clone", "history_string"],
         change="Intern allowed observation tokens and persistent (previous_history, token) links. Store history IDs on frontiers; materialize canonical public/private keys only when needed by the existing API.",
         caveat="This is different from the existing compiler's own-action recall interning. Preserve complete ordered observation and own-action history, including zero-prior worlds. Never key information sets by hidden State or only the latest observation; use equality after hashing and deterministic ordered public-group admission.",
         verification="Full semantic keys, Cursor lookup, beliefs, indistinguishability, action menu checks, tree/policy bits, RNG trajectory and budget/error parity; measure allocation count/bytes and RSS separately."),
    dict(id="S26e-shared-final-passes", priority=3, status="proposal-unimplemented",
         title="Reuse final average-policy value and counterfactual reach passes for growth scores",
         scope="same-result-computation",
         basis=["assess_value", "assess_reach", "scores_value", "scores_reach", "post_solve_scores"],
         change="Have final assessment retain average-policy values and both players' counterfactual reach; consume them in scores. Delay score construction until a next selection is needed.",
         caveat="The CFR working policy differs from its returned average policy. Reusing the last iteration's cache without recomputing under the average is wrong. Preserve floating-point order, information-constrained BR and underflow/error checks.",
         verification="Bitwise assessments, full policy, scores/selected frontier, terminal/limit/failed-admission results and errors. No changes to iteration limit, tolerance or check cadence."),
    dict(id="S26e-incremental-compilation", priority=4, status="proposal-unimplemented",
         title="Compile only changed/new tree structure on growth",
         scope="same-result-storage-and-validation",
         basis=["full_compile", "fresh_cfr"],
         change="Separate internal stable IDs from public canonical IDs, keep validated structure/recall summaries, compile newly appended nodes and changed frontier leaves.",
         caveat="Large implementation. Earlier leaf IDs can become decisions, shifting current information numbering. Must preserve DFS/member reduction order, changed utility scale, perfect recall validation and atomic last-committed result. This does not itself reuse CFR regrets.",
         verification="Compare every intermediate compiled snapshot, all policies, public keys, error order and committed counters with the reference, including failed admission."),
    dict(id="S26e-parallel-transition-batches", priority=5, status="proposal-unimplemented",
         title="Bounded CPU parallelism for independent engine transitions",
         scope="same-result-parallel",
         basis=["serial_transitions"],
         change="Specialize the immutable engine adapter for bounded Rayon batches over legal joint actions/worlds. Commit results serially in reference order; retain scalar fallback.",
         caveat="The current generic Domain allows stateful implementations; do not parallelize arbitrary Domain calls. Speculative calls can change attempted-work/error-budget semantics. Strict limit mode may need serial fallback or pre-reserved proven work. Bound simultaneous outcome memory.",
         verification="1/2/4-thread full output and error/budget parity, identical RNG selections, actual attempted-work accounting and RSS; report speedup against current one-thread optimized path."),
    dict(id="S26e-growth-solve-cadence", priority=6, status="research-unimplemented",
         title="Batch expansions or carry solver state between tree growth steps",
         scope="algorithm-change",
         basis=["fresh_cfr", "full_compile"],
         change="Study fewer intermediate CFR solves, adaptive checkpoint budgets, or justified warm starts as separate algorithms.",
         caveat="Changing a frontier changes its game/payoffs/scale. Carrying old regrets or average weights naively is not equivalent and can alter selection. Needs certificate versus the fixed final game and independent LP validation; partial-horizon gaps remain surrogate-only. Do not assert identical policy or win rate.",
         verification="Same time/work budget and reported fixed-game gap, independent LP cases, full versus partial horizon, deadlines/last solved snapshot. Existing exhaustive optimized backend is already preferable when full horizon is explicitly required in the measured fixtures."),
]

result = dict(
    schema=1, date_local="2026-10-03", status="analysis-only",
    source_sha="081124aaae5e0fe5aa1d8c5b9117803abd866875",
    run_id=37034440983, source_mutated=False, engine_executed=False,
    new_timing=False, new_ci_dispatched=False,
    evidence="Frozen source and existing successful GitHub artifact diagnostics; counts are not CPU time shares.",
    census=census, code_sites=sites, candidates=candidates,
    interpretation=[
        "Cumulative CFR iterations include smaller intermediate trees; never divide by final iterations to claim an elapsed-time speedup.",
        "Counts are identical in the old growing and optimized all-writer runs. No new workload was generated.",
        "No optimization candidate is adopted or implemented by this analysis. Existing OFF/default/R1/P15/P11 decisions remain unchanged.",
        "The sand tree has many terminal outcomes and few information sets; cached value backups already remove their value recomputation, but counterfactual reach still visits them. Any pruning would need to preserve underflow checks, so it is not the first proposed change.",
        "The Cursor repeated key formatting issue affects continuation lookup; that lookup is outside the current timed build+CFR benchmark, so fixing it cannot explain a faster benchmark kernel.",
    ],
    next_measurement=[
        "Count instruction materializations, reconstructed child states and bytes copied/formatted for observation histories in an observer-only build.",
        "Collect build/compile/CFR-assessment/selection stage times and RSS on GitHub; keep observers off for the final paired comparison.",
        "Use independent candidate switches, the existing 145 LP problems and new boundary counterexamples; do not count repeated runs as new test cases.",
    ],
)
out = HERE / "findings.json"
out.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
print(json.dumps(dict(output=str(out), cases=len(census), source_sites=len(sites), proposals=len(candidates), new_timing=False)))
