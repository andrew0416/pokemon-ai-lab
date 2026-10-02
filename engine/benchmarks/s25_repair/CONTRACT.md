# S25d repair comparison

Same seven original inputs, three seeds and deadlines as S25b. The baseline and
old selective engine use the OFF executable; revised uses the ON executable.
Both builds have the same source commit and six R1 features; only revised enables
`experiment-response-sweeps`. Defaults remain OFF; generic x86-64 is sufficient.

Sequential order rotates baseline/selective/revised, selective/revised/baseline,
revised/baseline/selective. The original independent reference and scorer are
unchanged. Full chance mass, action order, and terminal convention are identical.
No post-deadline policy is eligible. Process RSS includes setup and Python fork
overhead. The deterministic revised selector does not use the seed, so three runs
are timing replicates, not independent randomized searches.

After measured trials, half-hp-median also runs old and revised selective search
with a 120-second cap to distinguish finite-budget bias from completed-tree errors.
Exhausted frontier alone does not guarantee numerical convergence; propagated
reference intervals and actual BR gaps must be inspected before adoption.

Repair: root marginal-response sweeps with a positive uniform floor; exact reuse
when a changed entry has zero probability on both strategy axes or the child's
scalar value is bitwise unchanged; discard positions only at horizon/terminal
leaves or after every reachable continuation is complete. No action/chance pruning,
state merging, increase in node cap, hidden-state certificate, or production change.
