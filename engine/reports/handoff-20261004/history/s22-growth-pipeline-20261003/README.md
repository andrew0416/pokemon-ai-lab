# S22 / S26g growth pipeline experiment

Parent: frozen S21/S26f `b10b08477f196ce0b9b361fa7878e311fdf1d4c8`.

User requested stages 1–4 sequentially. Each stage passed its correctness gate before the next implementation. The final GitHub experiment measures isolated changes and all four together in one portable release binary.

1. Frontier node index and allocation-free chance selection (106 search tests).
2. Consume/reuse compiled snapshot storage, full validation retained (108 tests).
3. Incremental sequence payoff from replaced leaves, with scale refresh (112 tests).
4. Compressed internal BR filter and original-tree final certification (115 tests).

Production engine and original experimental paths are preserved; all four flags default OFF. AVX2 is not required. No local timing or win-rate measurement.

Frozen source: `70d5b0b463dc2cab2deb365c67ffcb2e2e7f95b8`, branch `codex/s26g-pipeline-20261003`.
GitHub Actions: https://github.com/andrew0416/pokemon-ai-lab/actions/runs/37123954718

Source is immutable after publication. Preparation scripts are one-shot records, not commands to rerun on the frozen folder. New revisions require another experiment snapshot.

See `source/engine/benchmarks/s26_pipeline/README.md` for contracts and benchmark definitions. `RESULTS.json` / `RESULTS.md` are written only after successful CI artifact verification.
