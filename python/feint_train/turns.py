import argparse
import copy
import json
import time
from dataclasses import dataclass, field
from pathlib import Path

import numpy as np
import torch
from feint_gym import TurnBatch

from feint_train.agent import DiscreteAgent
from feint_train.export import export_turn
from feint_train.rollout import Collector
from feint_train.update import UpdateConfig, ppo_update


@dataclass
class TurnConfig:
    game: str
    logdir: Path
    envs: int = 512
    updates: int = 200
    hours: float = 0.0
    mix: dict[str, float] = field(default_factory=lambda: {"learner": 1.0})
    evaluate: tuple[str, ...] = ()
    eval_every: int = 25
    eval_matches: int = 2000
    snapshot_every: int = 20
    archive: int = 16
    seed: int = 1
    lr: float = 3e-4
    gamma: float = 1.0
    lam: float = 0.95
    entropy: float = 0.003
    epochs: int = 4
    minibatches: int = 4
    resume: bool = False


def seat_plan(
    envs: int, mix: dict[str, float], archive: int, rng: np.random.Generator
) -> tuple[list[tuple[str, str]], np.ndarray]:
    weights = {k: v for k, v in mix.items() if v > 0 and (k != "frozen" or archive > 0)}
    if "frozen" in mix and archive == 0:
        weights["learner"] = weights.get("learner", 0.0) + mix["frozen"]
    names = list(weights)
    p = np.array([weights[n] for n in names], dtype=np.float64)
    picks = rng.choice(len(names), size=envs, p=p / p.sum())
    learner = np.arange(envs) % 2
    seats = []
    for env, pick in enumerate(picks):
        other = names[pick]
        if other == "frozen":
            other = f"frozen:{int(rng.integers(archive))}"
        pair = ("learner", other) if learner[env] == 0 else (other, "learner")
        seats.append(pair)
    return seats, learner


def slot_masks(seats: list[tuple[str, str]], learner: np.ndarray) -> dict[str, np.ndarray]:
    masks: dict[str, np.ndarray] = {}
    learner_mask = np.zeros(2 * len(seats), dtype=bool)
    for env, pair in enumerate(seats):
        learner_mask[2 * env + int(learner[env])] = True
        for side, name in enumerate(pair):
            if name == "learner":
                learner_mask[2 * env + side] = True
            elif name.startswith("frozen:"):
                masks.setdefault(name, np.zeros(2 * len(seats), dtype=bool))[2 * env + side] = True
    masks["learner"] = learner_mask
    return masks


def play(
    agent: DiscreteAgent,
    batch: TurnBatch,
    seeds: list[int],
    masks: dict[str, np.ndarray],
    frozen: dict[str, DiscreteAgent],
    collector: Collector | None,
) -> np.ndarray:
    envs = len(batch)
    slots = 2 * envs
    out = batch.reset(seeds)
    ended = out["terminated"].astype(bool)
    total = out["reward"].reshape(-1).astype(np.float64)
    zeros = np.zeros(slots, dtype=np.float32)
    while not ended.all():
        obs = out["obs"].reshape(slots, -1)
        legal = out["legal"].reshape(slots, -1)
        ready = out["ready"].reshape(-1).astype(bool)
        actions = np.full(slots, -1, dtype=np.int32)
        logps = np.zeros(slots, dtype=np.float32)
        values = np.zeros(slots, dtype=np.float32)
        mine = ready & masks["learner"]
        if mine.any():
            a, logp, value = agent.sample(obs[mine], legal[mine])
            actions[mine], logps[mine], values[mine] = a, logp, value
            if collector is not None:
                collector.act(mine, obs, actions.astype(np.float32), logps, values, legal)
        for name, mask in masks.items():
            theirs = ready & mask
            if name != "learner" and theirs.any():
                actions[theirs] = frozen[name].sample(obs[theirs], legal[theirs])[0]
        out = batch.step(actions.reshape(envs, 2))
        total += out["reward"].reshape(-1)
        ended |= out["terminated"].astype(bool)
        if collector is not None:
            after = out["obs"].reshape(slots, -1)
            next_values = agent.values(after) if collector.open else zeros
            collector.credit(
                {
                    "reward": out["reward"],
                    "ready": out["ready"],
                    "obs": out["obs"],
                    "final_obs": out["obs"],
                    "terminated": out["terminated"].astype(bool),
                    "truncated": np.zeros(envs, dtype=bool),
                },
                next_values,
                zeros,
            )
    return total


def check_opponents(game: str, opponents: tuple[str, ...]) -> None:
    for opponent in opponents:
        if opponent == "learner" or opponent.startswith("frozen:"):
            raise ValueError(f"evaluation needs a scripted opponent, not {opponent!r}")
        TurnBatch(game, [("learner", opponent)])


def check_mix(game: str, mix: dict[str, float]) -> None:
    for name in mix:
        if name in ("learner", "frozen"):
            continue
        if name.startswith("frozen:"):
            raise ValueError(f"the mix takes 'frozen', not {name!r}")
        try:
            TurnBatch(game, [("learner", name)])
        except ValueError as error:
            raise ValueError(f"the mix names an unknown seat {name!r}") from error


def evaluate(
    agent: DiscreteAgent, game: str, opponent: str, matches: int, seed: int
) -> dict[str, float]:
    check_opponents(game, (opponent,))
    seats = [("learner", opponent) if i % 2 == 0 else (opponent, "learner") for i in range(matches)]
    learner = np.arange(matches) % 2
    batch = TurnBatch(game, seats)
    seeds = [seed * 1_000_003 + i for i in range(matches)]
    rewards = play(agent, batch, seeds, slot_masks(seats, learner), {}, None).reshape(matches, 2)
    mine = rewards[np.arange(matches), learner]
    return {
        "mean_reward": float(mine.mean()),
        "wins": int((mine > 0).sum()),
        "draws": int((mine == 0).sum()),
        "losses": int((mine < 0).sum()),
        "matches": matches,
    }


def save(
    config: TurnConfig,
    agent: DiscreteAgent,
    optimizer: torch.optim.Optimizer,
    archive: list[dict[str, torch.Tensor]],
    update: int,
    rng: np.random.Generator,
    stats: dict[str, float],
) -> None:
    export_turn(agent, config.logdir / f"policy-{update:05d}.safetensors", config.game)
    with (config.logdir / "evals.jsonl").open("a") as log:
        for opponent in config.evaluate:
            result = evaluate(agent, config.game, opponent, config.eval_matches, update)
            log.write(json.dumps({"update": update, "opponent": opponent, **result}) + "\n")
        log.write(json.dumps({"update": update, "stats": stats}) + "\n")
    torch.save(
        {
            "agent": agent.state_dict(),
            "optimizer": optimizer.state_dict(),
            "archive": archive,
            "update": update,
            "rng": rng.bit_generator.state,
            "torch_rng": torch.get_rng_state(),
        },
        config.logdir / "checkpoint.pt",
    )


def train(config: TurnConfig) -> DiscreteAgent:
    check_opponents(config.game, config.evaluate)
    check_mix(config.game, config.mix)
    torch.manual_seed(config.seed)
    rng = np.random.default_rng(config.seed)
    probe = TurnBatch(config.game, [("learner", "learner")])
    agent = DiscreteAgent(np.array(probe.scales, dtype=np.float32), probe.actions)
    optimizer = torch.optim.Adam(agent.parameters(), lr=config.lr)
    archive: list[dict[str, torch.Tensor]] = []
    update = 0
    checkpoint = config.logdir / "checkpoint.pt"
    if config.resume and checkpoint.exists():
        state = torch.load(checkpoint, weights_only=True)
        agent.load_state_dict(state["agent"])
        optimizer.load_state_dict(state["optimizer"])
        archive = state["archive"]
        update = state["update"]
        rng.bit_generator.state = state["rng"]
        torch.set_rng_state(state["torch_rng"])
    config.logdir.mkdir(parents=True, exist_ok=True)
    settings = UpdateConfig(
        epochs=config.epochs, minibatches=config.minibatches, entropy=config.entropy
    )
    started = time.monotonic()
    saved = update
    while update < config.updates:
        if config.hours and time.monotonic() - started > config.hours * 3600:
            break
        seats, learner = seat_plan(config.envs, config.mix, len(archive), rng)
        masks = slot_masks(seats, learner)
        frozen = {}
        for name in masks:
            if name.startswith("frozen:"):
                copy_agent = copy.deepcopy(agent)
                copy_agent.load_state_dict(archive[int(name.split(":")[1])])
                frozen[name] = copy_agent
        collector = Collector(
            slots=2 * config.envs, gamma=config.gamma, potential_scale=1.0, turns=True
        )
        seeds = [int(s) for s in rng.integers(0, 2**62, config.envs)]
        play(agent, TurnBatch(config.game, seats), seeds, masks, frozen, collector)
        advantage, returns = collector.advantages(config.lam)
        stats = ppo_update(agent, optimizer, collector.arrays(), advantage, returns, settings, rng)
        update += 1
        if update % config.snapshot_every == 0:
            archive.append(copy.deepcopy(agent.state_dict()))
            archive = archive[-config.archive :]
        if update % config.eval_every == 0 or update == config.updates:
            save(config, agent, optimizer, archive, update, rng, vars(stats))
            saved = update
    if update > saved:
        save(config, agent, optimizer, archive, update, rng, {})
    return agent


def parse_mix(text: str) -> dict[str, float]:
    mix = {}
    for part in text.split(","):
        name, _, share = part.rpartition("=")
        mix[name] = float(share)
    return mix


def main() -> None:
    parser = argparse.ArgumentParser(description="Train a turn-based game policy with PPO")
    parser.add_argument("--game", required=True, choices=["kuhn", "believe"])
    parser.add_argument("--logdir", type=Path, required=True)
    parser.add_argument("--envs", type=int, default=512)
    parser.add_argument("--updates", type=int, default=200)
    parser.add_argument("--hours", type=float, default=0.0)
    parser.add_argument("--mix", default="learner=1")
    parser.add_argument("--evaluate", default="")
    parser.add_argument("--eval-every", type=int, default=25)
    parser.add_argument("--eval-matches", type=int, default=2000)
    parser.add_argument("--seed", type=int, default=1)
    parser.add_argument("--lr", type=float, default=3e-4)
    parser.add_argument("--entropy", type=float, default=0.003)
    parser.add_argument("--resume", action="store_true")
    args = parser.parse_args()
    train(
        TurnConfig(
            game=args.game,
            logdir=args.logdir,
            envs=args.envs,
            updates=args.updates,
            hours=args.hours,
            mix=parse_mix(args.mix),
            evaluate=tuple(o for o in args.evaluate.split(",") if o),
            eval_every=args.eval_every,
            eval_matches=args.eval_matches,
            seed=args.seed,
            lr=args.lr,
            entropy=args.entropy,
            resume=args.resume,
        )
    )


if __name__ == "__main__":
    main()
