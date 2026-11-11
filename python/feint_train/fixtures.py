import argparse
import struct
from pathlib import Path

import numpy as np
import torch
from feint_gym import HockeyBatch

from feint_train.agent import Agent
from feint_train.export import export


def write_matrix(path: Path, data: np.ndarray) -> None:
    rows, width = data.shape
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = np.ascontiguousarray(data, dtype="<f4").tobytes()
    path.write_bytes(struct.pack("<II", rows, width) + payload)


def read_matrix(path: Path) -> np.ndarray:
    raw = path.read_bytes()
    rows, width = struct.unpack("<II", raw[:8])
    return np.frombuffer(raw[8:], dtype="<f4").reshape(rows, width).astype(np.float32)


def load_agent(checkpoint: Path) -> Agent:
    agent = Agent(np.array(HockeyBatch.scales, dtype=np.float32))
    agent.load_state_dict(torch.load(checkpoint, weights_only=True)["agent"])
    return agent


def extreme_features(scales: np.ndarray) -> np.ndarray:
    width = len(scales)
    rows = [np.zeros(width, dtype=np.float32)]
    for factor in (1.0, -1.0, 10.0, -10.0):
        rows.append((factor * scales).astype(np.float32))
    for i in range(width):
        for sign in (1.0, -1.0):
            one = np.zeros(width, dtype=np.float32)
            one[i] = sign * scales[i]
            rows.append(one)
    return np.stack(rows)


def record_features(agent: Agent, count: int, seed: int, threads: int) -> np.ndarray:
    opponents = ["level:3", "level:5"]
    seats = [
        ("learner", opponents[i % 2]) if (i // 2) % 2 == 0 else (opponents[i % 2], "learner")
        for i in range(16)
    ]
    batch = HockeyBatch(seats, seed=seed, threads=threads)
    out = batch.reset()
    seen: list[np.ndarray] = []
    while len(seen) < count * 8:
        actions = np.zeros((len(seats), 2, 2), dtype=np.float32)
        ready = out["ready"]
        if ready.any():
            obs = out["obs"][ready]
            actions[ready] = agent.act(obs)
            seen.extend(obs)
        out = batch.step(actions)
    pool = np.stack(seen)
    picked = pool[np.linspace(0, len(pool) - 1, count).astype(int)]
    return np.ascontiguousarray(picked, dtype=np.float32)


def make_fixtures(
    checkpoint: Path, out: Path, policy: Path, recorded: int, seed: int, threads: int
) -> None:
    agent = load_agent(checkpoint)
    export(agent, policy)
    scales = np.array(HockeyBatch.scales, dtype=np.float32)
    inputs = np.concatenate(
        [record_features(agent, recorded, seed, threads), extreme_features(scales)]
    )
    write_matrix(out / "features.bin", inputs)
    write_matrix(out / "outputs.bin", agent.act(inputs))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--checkpoint", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--policy", type=Path, required=True)
    parser.add_argument("--recorded", type=int, default=1000)
    parser.add_argument("--seed", type=int, default=4242)
    parser.add_argument("--threads", type=int, default=4)
    args = parser.parse_args()
    make_fixtures(args.checkpoint, args.out, args.policy, args.recorded, args.seed, args.threads)


if __name__ == "__main__":
    main()
