# Teams rebuilt from public Showdown replays (V13-replay-parity, 2026-09-28)

Source: 200 public replays of `gen9championsvgc2026regmc` (replay.pokemonshowdown.com, newest first on
2026-09-28; ids, URLs and upload times in `replays.json`; player names are not kept). Collected by
`engine/scripts/replay_fetch.py`, parsed by `engine/scripts/replay_parse.py`.

`games/<id>.json` is one game as `lab-replay` reads it: both brought teams, the logged decisions and the
observation after each. **The sets are not the players' sets.** Open team sheets (4 games) give species,
item, ability, moves and nature; otherwise only what the log revealed (moves used, items and abilities
shown), filled with assumptions: unrevealed ability = the species' first one that does not announce
itself on entry, unrevealed item = none (Light Clay when a screen outlasted 5 turns), a filler Rest for
turns a Pokémon did not act, unseen brought members from team preview with a placeholder set, and
**Stat Points always assumed** (HP 32 / attack 32 / Speed 2, then fitted per game by `lab-replay
--fit-sp` against the log; the fitted spreads are in the positions and in the run's `checks/`).
Use these only as parity-test material, never as team data.
