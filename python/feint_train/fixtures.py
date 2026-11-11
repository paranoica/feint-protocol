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


def zero_forcing_features(agent: Agent, scales: np.ndarray, units: int) -> np.ndarray:
    weight = agent.actor[0].weight.detach().cpu().numpy().astype(np.float32)
    bias = agent.actor[0].bias.detach().cpu().numpy().astype(np.float32)
    inverse = (np.float32(1.0) / scales).astype(np.float32)
    rows = []
    for j in range(min(units, len(bias))):
        k = int(np.argmax(np.abs(weight[j])))
        w = weight[j, k]
        if w == 0:
            continue
        found = None
        z = np.float32(-bias[j] / w)
        for _ in range(64):
            if np.float32(bias[j] + np.float32(w * z)) == 0:
                found = z
                break
            z = np.nextafter(z, np.float32(np.inf), dtype=np.float32)
        if found is None:
            continue
        x = np.float32(found * scales[k])
        for step in (np.float32(np.inf), np.float32(-np.inf)):
            probe = x
            for _ in range(64):
                if np.float32(probe * inverse[k]) == found:
                    row = np.zeros(len(scales), dtype=np.float32)
                    row[k] = probe
                    rows.append(row)
                    break
                probe = np.nextafter(probe, step, dtype=np.float32)
            else:
                continue
            break
    return np.stack(rows) if rows else np.zeros((0, len(scales)), dtype=np.float32)


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
        [
            record_features(agent, recorded, seed, threads),
            zero_forcing_features(agent, scales, units=64),
            extreme_features(scales),
        ]
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
