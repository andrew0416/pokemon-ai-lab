# Prior accuracy gate for old4+D versus old4+DEF

This gate reuses two immutable broad-accuracy results through their identical
old4 baseline. It does **not** claim a new direct D-versus-DEF broad execution.
Fresh regressions, observers, differential probes, and same-runner timing remain
required by the paired job.

| Evidence | Actions run / controller | Required relevant success | Broad contract |
|---|---|---|---|
| D | 36681590730 / `6dcc56a6398702c2281019dd00fe5ee4eb7a2019` | All agreement jobs and speed replay-action-keys | 6,184 successes; zero error/different/uncompared; exact-v1 against old4 |
| DEF | 36698588329 / `7bfb3380cc12439d368a4a288d00fe1269c00455` | All agreement jobs and speed p8def-combined | Same 6,184 denominator; existing Slot-diff semantic contract against old4 |

The first run's overall conclusion is failure because the separate F speed job
failed. The gate accepts only its pinned successful agreement and D speed jobs;
it does not turn that global failure into a success. DEF's run must be successful.
Both runs must be attempt 1, completed workflow_dispatch runs in the exact
repository at the pinned heads. Exact job IDs, source SHA048, artifact IDs/archive
digests, and immutable agreement Git tree IDs are checked.

`controller-pins.json` was generated read-only from those two local immutable Git
commits. Its own SHA-256 is pinned in the module. It records every agreement file's
SHA-256/Git blob ID, both variant documents, and 15 unchanged corpus/selection/probe/
contract files. The current inherited agreement controller must match DEF's pins.
The actual paired `ci.feature_args('p8d-vs-p8def', label)` must request exactly
old4+D and old4+DEF, with timing observers OFF and P11 absent.

Run the gate from a fresh receipt directory (normally the workflow checkout):

```sh
GITHUB_TOKEN=... python engine/benchmarks/p8d_vs_p8def/prior_agreement.py --remote
# Download only each pinned run's agreement-summary artifact into separate paths.
python engine/benchmarks/p8d_vs_p8def/prior_agreement.py --summaries \
  prior-agreement/d/agreement-summary.json \
  prior-agreement/def/agreement-summary.json
```

`--remote` performs read-only metadata/Git-tree GETs and writes
`prior-agreement-remote.json`; it never downloads variant output archives.
`--summaries` revalidates that remote record, requires the exact downloaded summary
bytes, and runs the original independent/combined contract validators. It writes
`prior-agreement-receipt.json`, retaining all 1,505 raw instruction-difference IDs
and DEF's corresponding baseline/candidate SHA/byte evidence. Their IDs must also
match the original P8e ledger. Both outputs use exclusive creation and cannot
overwrite earlier receipts. The source checker is also public as `validate_source()`
for the dispatch helper.

Only outcome instruction text has the existing P8e semantic exception. Full state,
hidden payload, probabilities, order, errors, rollback/hash evidence and search/
oracle contracts remain those of the frozen probes. The transitive relationship is
limited to those frozen inputs and contracts; it is not a fresh pairwise proof or
a claim about all engine states.
