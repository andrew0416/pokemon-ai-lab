# Optimization agreement evaluation, 2026-09-30

This evaluates correctness, not speed. No elapsed-time performance claim is
derived from simultaneous GitHub jobs. Production engine code is unchanged.

Eight immutable builds cover the pre-P8g feature-off baseline, P8g, and P9/P8c/P10
individually enabled and disabled with P8g held constant. See `variants.json`.
The independent candidates are not silently combined. P9/P8c observer features
are enabled only to demonstrate that their optimized paths actually execute;
observer records are excluded from logical-output equality and retained as
separate evidence. All builds use generic x86-64, with no AVX2 requirement.

The original Showdown corpus contains 1,781 position IDs, 2,052 reports and six
explicitly excluded stale pins. IDs are not synonymous with unique states.
The manifest records raw-file and canonical-before duplicates, games, matchups,
turn stages, report modes and actual feature coverage. Original gzip files,
source metadata, hashes and exclusions are preserved unchanged.

Each executable replays the frozen inputs afresh. Oracle checks use full
canonical state keys and a probability tolerance of 1e-9. Differential probes
compare SHA-256 over their complete serialized output, including finite float
bits, outcome/instruction ordering, hidden state, errors and state restoration.
Search selection is determined before execution from distinct canonical
positions, never from which cases happened to pass or run quickly. Search is
bounded externally; timeouts and failed/incomplete outputs remain explicitly
uncompared. Matching refusal/error records are reported separately from normal
successful results. An equal error is not evidence of supported gameplay.

The search sample exercises public maximin and mixed search at depth 1, with
a smaller depth-2 set, both sides, Expect/Worst, different roll modes and
one/two workers. Main corpus decisions cover turn and replacement boundaries;
unsupported suspended-search inputs are not misrepresented as completed
searches. No existing verdict or prebuilt executable is reused. All input,
source, feature and binary fingerprints accompany the fresh result records.

The historical corpus is a regression dataset, not an unseen held-out proof
of all game rules. A successful comparison establishes agreement on the
reported inputs and configurations only. Python bindings, future mechanics,
all possible hidden states, and combinations of independent optimizations
still require their own validation.

## Explicit fixture contracts (follow-up to run 36633204923)

The original run and its failed final gate remain unchanged. A new frozen plan
keeps the same case IDs and denominator, but selects these contracts before any
variant executes:

- Three `rr-*-undecided-gender` reports are complete uniform gender mixtures.
  `ci_agreement_oracle` checks every one of the `2^k` assignments and its weight,
  applies/reverses each turn, and compares the weighted canonical distribution.
  Duplicate, missing, unresolved, or incorrectly weighted genders fail the case.
- `ss-redirect-tie-hidden-order` contains two equally probable hidden ability
  orders with identical canonical before states. Both histories are checked
  separately against independent full Showdown enumerations. The existing
  report covers Rod B; `data/contracts/ss-redirect-tie-hidden-order-rod-a.*`
  covers Rod A after changing only the setup PRNG seed. Its generator and
  provenance record the pinned Showdown revision, seed trials and file hashes.
  This is not an assumed symmetry and does not replace the original report.
- Five fixture turn probes (`bb-choicelock-struggle`, `red-card-drag-update`,
  `hyper-beam-recharge`, `u-truant-recharge`, `nn-pressure-locked-outrage`) use
  their existing report's `before` to identify the requested parents. Every
  other setup parent is still executed with the original choice and retained
  as additional evidence. Schema 2 records counts, restoration, errors and
  status separately for requested/additional/global scopes. Only the known
  exact out-of-domain choice errors in additional parents are classified as
  expected rejections. A new error, unsupported rule, failed restoration,
  missing parent, incomplete output or empty enumeration cannot pass.

Oracle contract records retain every per-history canonical distribution and
probability; agreement comparisons include those records. Scoped turn stream
hashes still include all additional outcomes and rejected choices. Summary
fields `oracle_contracts` and `scoped_turn_coverage` make the validation scope
visible: success of a requested fixture turn does not claim that its recorded
choice is valid for every other setup outcome. Normal oracle/turn cases keep
their previous comparator and version-1 probe contract. The two Future Sight
unsupported scenarios are not converted into successful contracts here.
