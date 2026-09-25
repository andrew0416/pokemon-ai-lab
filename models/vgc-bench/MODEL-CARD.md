---
license: mit
pipeline_tag: reinforcement-learning
tags:
- reinforcement-learning
- tensorboard
---

This repo stores `.zip` checkpoints and TensorBoard logs from [vgc-bench](https://github.com/cameronangliss/vgc-bench).
The training methods are described in [VGC-Bench: Towards Mastering Diverse Team Strategies in Competitive Pokemon](https://arxiv.org/abs/2506.10326).

## Repository Structure

### `results/` — Main benchmark results

Results from the main VGC-Bench experiments with varied team pool sizes.

- **`saves_bc/`** — Behavior cloning (BC) checkpoints
  - `seed1/` — BC policy checkpoints (epochs 0–100)
- **`logs_bc/`** — BC TensorBoard logs
  - `seed1/`
- **`saves_bc_sp/`** — BC-initialized self-play (BCSP) checkpoints
  - `reg_g/{1,4,16,64}_teams/seed1/` — BCSP checkpoints across team pool sizes
- **`logs_bc_sp/`** — BCSP TensorBoard logs
  - `reg_g/{1,4,16,64}_teams/seed1_0/`

### `results_<tournament>_finals/` — VGC tournament matchup results

Each directory contains results from training on a specific VGC tournament finals matchup, with `team1.txt` and `team2.txt` defining the two teams.

| Directory | Method | Reg | Teams |
|---|---|---|---|
| `results_worlds_2024_finals/` | BC + self-play, no mirrors | g | 2 |
| `results_atlanta_2025_finals/` | BC + self-play, no mirrors | g | 2 |
| `results_euic_2025_finals/` | BC + self-play, no mirrors | g | 2 |
| `results_san_antonio_2025_finals/` | BC + self-play, no mirrors | g | 2 |
| `results_euic_2026_finals/` | BC + self-play, no mirrors | f | 2 |

Each contains:
- **`saves_bc_sp_xm/`** — BCSP checkpoints (no mirror matches)
  - `reg_{g,f}/{N}_teams/seed1/`
- **`logs_bc_sp_xm/`** — TensorBoard logs
  - `reg_{g,f}/{N}_teams/seed1_0/`
- **`team1.txt`**, **`team2.txt`** — Team strings for the matchup

## Naming Conventions

- **bc** — behavior cloning
- **sp** — self-play
- **xm** — excluding mirror matches
- **reg_g/reg_f** — VGC regulation (e.g. Regulation G, Regulation F)

## Usage

Checkpoints are Stable Baselines 3 `.zip` files loadable with:

```python
from stable_baselines3 import PPO
model = PPO.load("path/to/checkpoint.zip")
```

See the [vgc-bench README](https://github.com/cameronangliss/vgc-bench) for full training and evaluation instructions.

## Cite us

```bibtex
@inproceedings{anglissvgc,
  title={VGC-Bench: Towards Mastering Diverse Team Strategies in Competitive Pok{\'e}mon},
  author={Angliss, Cameron L and Cui, Jiaxun and Hu, Jiaheng and Rahman, Arrasy and Stone, Peter},
  booktitle={The 25th International Conference on Autonomous Agents and Multi-Agent Systems}
}
```
