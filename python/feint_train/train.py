import argparse
import os
import time
from collections import defaultdict, deque
from dataclasses import asdict, dataclass, field
from pathlib import Path

import numpy as np
import torch
from feint_gym import HockeyBatch
from torch.utils.tensorboard import SummaryWriter

from feint_train.agent import Agent
from feint_train.evaluate import evaluate
from feint_train.export import export
from feint_train.league import League, match_value
from feint_train.rollout import Collector
from feint_train.update import UpdateConfig, UpdateStats, ppo_update


@dataclass
class TrainConfig:
    opponents: list[str] = field(default_factory=lambda: ["level:1"])
    envs: int = 256
    threads: int = 6
    torch_threads: int = 2
    device: str = "cpu"
    steps: int = 128
    minutes: float = 30.0
    seed: int = 1
    lr: float = 3e-4
    gamma: float = 0.999
    lam: float = 0.95
    std: float = 0.3
    shaping: float = 0.1
    shaping_until: float = 0.5
    eval_every_minutes: float = 5.0
    eval_matches: int = 50
    eval_opponents: list[str] = field(default_factory=lambda: ["level:1"])
    eval_seed: int = 77
    league: bool = False
    archive_size: int = 12
    generation_minutes: float = 15.0
    snapshot_minutes: float = 30.0
    update: UpdateConfig = field(default_factory=UpdateConfig)


def seats(config: TrainConfig) -> list[tuple[str, str]]:
    out = []
    for index in range(config.envs):
        opponent = config.opponents[index % len(config.opponents)]
        out.append(
            ("learner", opponent)
            if (index // len(config.opponents)) % 2 == 0
            else (opponent, "learner")
        )
    return out


class Stats:
    def __init__(self, window: int = 400) -> None:
        self.results: dict[str, deque[int]] = defaultdict(lambda: deque(maxlen=window))
        self.goals: deque[float] = deque(maxlen=window)

    def record(self, opponent: str, mine: int, theirs: int, finished: bool) -> None:
        self.results[opponent].append(1 if finished and mine > theirs else 0)
        self.goals.append(mine - theirs)


class Trainer:
    def __init__(
        self,
        config: TrainConfig,
        logdir: Path,
        resume: Path | None = None,
        init: Path | None = None,
    ) -> None:
        if resume is not None and init is not None:
            raise ValueError("resume a run or start from a checkpoint, not both")
        if config.seed == config.eval_seed:
            raise ValueError("training and evaluation need different seeds")
        torch.set_num_threads(config.torch_threads)
        torch.manual_seed(config.seed)
        self.config = config
        self.logdir = logdir
        self.rng = np.random.default_rng(config.seed)
        self.envs = config.envs
        self.slots = self.envs * 2
        scales = np.array(HockeyBatch.scales, dtype=np.float32)
        self.agent = Agent(scales, std=config.std).to(torch.device(config.device))
        self.optimizer = torch.optim.Adam(self.agent.parameters(), lr=config.lr, eps=1e-5)
        self.league = League(archive_size=config.archive_size) if config.league else None
        spent = 0.0
        self.updates = 0
        self.generation = 0
        state = None
        if resume is not None:
            state = torch.load(resume, map_location=config.device, weights_only=True)
            self.agent.load_state_dict(state["agent"])
            self.optimizer.load_state_dict(state["optimizer"])
            spent = state["spent"]
            self.updates = state["updates"]
            self.generation = state.get("generation", 0) + 1
            if self.league is not None and state.get("league") is not None:
                self.league = League.from_state(state["league"])
        if init is not None:
            weights = torch.load(init, map_location=config.device, weights_only=True)["agent"]
            self.agent.load_state_dict(weights)
        self.writer = SummaryWriter(str(logdir))
        self.writer.add_text("config", str(asdict(config)))
        self.collector = Collector(
            slots=self.slots, gamma=config.gamma, potential_scale=float(scales[0])
        )
        self.stats = Stats()
        self.start = time.perf_counter() - spent
        now = self.elapsed()
        self.next_eval = now + config.eval_every_minutes * 60.0
        self.next_generation = now + config.generation_minutes * 60.0
        self.next_snapshot = now + config.snapshot_minutes * 60.0
        if state is not None:
            self.next_eval = state.get("next_eval", self.next_eval)
            self.next_snapshot = state.get("next_snapshot", self.next_snapshot)
            if "numpy_rng" in state:
                self.rng.bit_generator.state = state["numpy_rng"]
        self.build_batch()
        if state is not None and "torch_rng" in state:
            torch.set_rng_state(state["torch_rng"])

    def build_batch(self) -> None:
        self.batch_seed = self.config.seed + 7919 * self.generation
        if self.league is not None:
            table = self.league.table(self.envs, self.rng)
            self.table = table.seats
            self.opponent_of = table.opponents
            frozen = table.frozen
        else:
            self.table = seats(self.config)
            self.opponent_of = [b if a == "learner" else a for a, b in self.table]
            frozen = []
        self.batch = HockeyBatch(
            self.table, seed=self.batch_seed, threads=self.config.threads, frozen=frozen
        )
        self.out = self.batch.reset()

    def next_generation_now(self) -> None:
        self.generation += 1
        self.build_batch()
        self.next_generation = self.elapsed() + self.config.generation_minutes * 60.0

    def snapshot(self) -> None:
        if self.league is None:
            return
        name = f"snap-{self.updates:05d}"
        path = self.logdir / "archive" / f"{name}.safetensors"
        export(self.agent, path)
        self.league.add(name, path)

    def elapsed(self) -> float:
        return time.perf_counter() - self.start

    def save(self) -> None:
        path = self.logdir / "checkpoint.pt"
        partial = path.with_suffix(".pt.partial")
        torch.save(
            {
                "agent": self.agent.state_dict(),
                "optimizer": self.optimizer.state_dict(),
                "spent": self.elapsed(),
                "updates": self.updates,
                "generation": self.generation,
                "next_eval": self.next_eval,
                "next_snapshot": self.next_snapshot,
                "league": None if self.league is None else self.league.state(),
                "torch_rng": torch.get_rng_state(),
                "numpy_rng": self.rng.bit_generator.state,
                "config": asdict(self.config),
            },
            partial,
        )
        partial.replace(path)

    def checkpoint(self, name: str) -> None:
        self.save()
        export(self.agent, self.logdir / f"{name}.safetensors")

    def evaluate(self) -> None:
        for opponent in self.config.eval_opponents:
            result = evaluate(
                self.agent.act,
                opponent,
                self.config.eval_matches,
                self.config.eval_seed,
                self.config.threads,
            )
            self.writer.add_scalar(f"eval/{opponent}", result.share, self.updates)
            self.writer.add_scalar(f"eval_unfinished/{opponent}", result.unfinished, self.updates)
            self.writer.add_scalar(
                f"eval_ahead_at_cap/{opponent}", result.ahead_at_cap, self.updates
            )
            self.writer.add_scalar(
                f"eval_goals_per_match/{opponent}",
                (result.goals_for + result.goals_against) / max(1, result.matches),
                self.updates,
            )
            print(
                f"[{self.elapsed() / 60:5.1f} min, update {self.updates}] eval vs {opponent}: "
                f"{result.wins}/{result.matches} won, {result.losses} lost, "
                f"{result.unfinished} unfinished ({result.ahead_at_cap} ahead, "
                f"{result.behind_at_cap} behind), goals {result.goals_for}:{result.goals_against}",
                flush=True,
            )

    def values_at(self, obs: np.ndarray, mask: np.ndarray) -> np.ndarray:
        values = np.zeros(self.slots, dtype=np.float32)
        if mask.any():
            values[mask] = self.agent.values(obs.reshape(self.slots, -1)[mask])
        return values

    def step(self) -> None:
        out = self.out
        ready = out["ready"].reshape(-1)
        obs = out["obs"].reshape(self.slots, -1)
        action = np.zeros((self.slots, 2), dtype=np.float32)
        logp = np.zeros(self.slots, dtype=np.float32)
        value = np.zeros(self.slots, dtype=np.float32)
        if ready.any():
            action[ready], logp[ready], value[ready] = self.agent.sample(obs[ready])
        self.collector.act(ready, obs, action, logp, value)
        out = self.batch.step(action.reshape(self.envs, 2, 2))
        values = self.values_at(out["obs"], out["ready"].reshape(-1))
        final_values = self.values_at(out["final_obs"], np.repeat(out["truncated"], 2))
        self.collector.credit(out, values, final_values)
        for env in np.flatnonzero(out["terminated"] | out["truncated"]):
            me = 0 if self.table[env][0] == "learner" else 1
            mine, theirs = int(out["score"][env, me]), int(out["score"][env, 1 - me])
            finished = bool(out["terminated"][env])
            opponent = self.opponent_of[env]
            if self.league is not None and self.league.find(opponent) is not None:
                self.league.record(opponent, match_value(mine, theirs, finished))
                opponent = "frozen"
            self.stats.record(opponent, mine, theirs, finished)
        self.out = out

    def iteration(self, progress: float) -> None:
        config = self.config
        for group in self.optimizer.param_groups:
            group["lr"] = config.lr * max(0.05, 1.0 - progress)
        self.collector.shaping = config.shaping * max(0.0, 1.0 - progress / config.shaping_until)
        began = time.perf_counter()
        for _ in range(config.steps):
            self.step()
        rollout = time.perf_counter() - began
        advantage, returns = self.collector.advantages(config.lam)
        data = self.collector.arrays()
        began = time.perf_counter()
        result = ppo_update(
            self.agent, self.optimizer, data, advantage, returns, config.update, self.rng
        )
        update = time.perf_counter() - began
        self.collector.clear()
        self.updates += 1
        self.log(result, rollout, update, len(data["obs"]))

    def log(self, result: UpdateStats, rollout: float, update: float, transitions: int) -> None:
        w, n = self.writer, self.updates
        speed = self.envs * self.config.steps / (rollout + update)
        for name, number in asdict(result).items():
            w.add_scalar(f"loss/{name}", number, n)
        w.add_scalar("policy/std", float(self.agent.log_std.detach().exp().mean()), n)
        w.add_scalar("speed/env_steps_per_s", speed, n)
        w.add_scalar("speed/update_s", update, n)
        w.add_scalar("speed/rollout_s", rollout, n)
        w.add_scalar("train/transitions", transitions, n)
        w.add_scalar("train/shaping", self.collector.shaping, n)
        won = {k: float(np.mean(v)) for k, v in self.stats.results.items() if v}
        for opponent, share in won.items():
            w.add_scalar(f"train/won_{opponent}", share, n)
        if self.stats.goals:
            w.add_scalar("train/goal_difference", float(np.mean(self.stats.goals)), n)
        if n % 10 == 0:
            print(
                f"[{self.elapsed() / 60:5.1f} min] update {n}: {speed:,.0f} env-steps/s "
                f"(rollout {rollout:.1f}s, update {update:.1f}s), "
                f"won {', '.join(f'{k} {v:.2f}' for k, v in won.items())}",
                flush=True,
            )

    def run(self) -> Agent:
        config = self.config
        budget = config.minutes * 60.0
        evaluated = -1
        while self.elapsed() < budget:
            self.iteration(self.elapsed() / budget)
            if self.elapsed() >= self.next_eval:
                self.evaluate()
                evaluated = self.updates
                self.next_eval += config.eval_every_minutes * 60.0
                self.checkpoint(f"policy-{self.updates:05d}")
            if self.league is not None and self.elapsed() >= self.next_snapshot:
                self.snapshot()
                self.next_snapshot += config.snapshot_minutes * 60.0
            if self.league is not None and self.elapsed() >= self.next_generation:
                self.next_generation_now()
                self.save()
        if evaluated != self.updates:
            self.evaluate()
        self.checkpoint("policy-final")
        self.writer.close()
        return self.agent


def train(
    config: TrainConfig, logdir: Path, resume: Path | None = None, init: Path | None = None
) -> Agent:
    return Trainer(config, logdir, resume, init).run()


def hide_gpu_for(device: str) -> None:
    if device == "cpu":
        os.environ["CUDA_VISIBLE_DEVICES"] = ""


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--logdir", type=Path, required=True)
    parser.add_argument("--opponents", default="level:1")
    parser.add_argument("--eval-opponents", default="level:1")
    parser.add_argument("--minutes", type=float, default=30.0)
    parser.add_argument("--envs", type=int, default=256)
    parser.add_argument("--threads", type=int, default=6)
    parser.add_argument("--torch-threads", type=int, default=2)
    parser.add_argument("--device", default="cpu")
    parser.add_argument("--eval-every", type=float, default=5.0)
    parser.add_argument("--eval-matches", type=int, default=50)
    parser.add_argument("--seed", type=int, default=1)
    parser.add_argument("--gamma", type=float, default=TrainConfig.gamma)
    parser.add_argument("--shaping", type=float, default=TrainConfig.shaping)
    parser.add_argument("--resume")
    parser.add_argument("--init", type=Path)
    parser.add_argument("--hours", type=float)
    parser.add_argument("--lr", type=float, default=TrainConfig.lr)
    parser.add_argument("--steps", type=int, default=TrainConfig.steps)
    parser.add_argument("--league", action="store_true")
    parser.add_argument("--generation-minutes", type=float, default=TrainConfig.generation_minutes)
    parser.add_argument("--snapshot-minutes", type=float, default=TrainConfig.snapshot_minutes)
    args = parser.parse_args()
    hide_gpu_for(args.device)
    resume = resolve_resume(args.resume, args.logdir)
    init = None if resume is not None else args.init
    config = TrainConfig(
        opponents=args.opponents.split(","),
        eval_opponents=args.eval_opponents.split(","),
        minutes=args.hours * 60.0 if args.hours is not None else args.minutes,
        envs=args.envs,
        threads=args.threads,
        torch_threads=args.torch_threads,
        device=args.device,
        eval_every_minutes=args.eval_every,
        eval_matches=args.eval_matches,
        seed=args.seed,
        gamma=args.gamma,
        shaping=args.shaping,
        lr=args.lr,
        steps=args.steps,
        league=args.league,
        generation_minutes=args.generation_minutes,
        snapshot_minutes=args.snapshot_minutes,
    )
    train(config, args.logdir, resume, init)


def resolve_resume(resume: str | None, logdir: Path) -> Path | None:
    if resume is None:
        return None
    if resume == "auto":
        path = logdir / "checkpoint.pt"
        return path if path.exists() else None
    return Path(resume)


if __name__ == "__main__":
    main()
