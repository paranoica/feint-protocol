import argparse
import time

import numpy as np
from feint_gym import HockeyBatch


def mix(envs: int) -> list[tuple[str, str]]:
    level5 = envs // 10
    hand = envs // 5 - level5
    human = envs // 10
    readers = envs // 10
    seats = [("learner", "level:5")] * level5
    seats += [("learner", f"level:{1 + i % 4}") for i in range(hand)]
    seats += [("learner", "human")] * human
    seats += [("learner", "reader:anticipating" if i % 2 else "reader") for i in range(readers)]
    seats += [("learner", "learner")] * (envs - len(seats))
    return seats


def measure(envs: int, threads: int, steps: int, seed: int) -> float:
    batch = HockeyBatch(mix(envs), seed=seed, threads=threads)
    rng = np.random.default_rng(seed)
    out = batch.reset()
    for _ in range(20):
        out = batch.step(rng.uniform(-1, 1, (envs, 2, 2)).astype(np.float32))
    decisions = 0
    start = time.perf_counter()
    for _ in range(steps):
        out = batch.step(rng.uniform(-1, 1, (envs, 2, 2)).astype(np.float32))
        decisions += int(out["ready"].sum())
    elapsed = time.perf_counter() - start
    rate = steps * envs / elapsed
    print(
        f"envs {envs} threads {threads}: {rate:,.0f} env-steps/s, "
        f"{decisions / elapsed:,.0f} learner decisions/s, {rate / 60:,.0f}x realtime"
    )
    return rate


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--envs", type=int, default=256)
    parser.add_argument("--threads", type=int, nargs="+", default=[6])
    parser.add_argument("--steps", type=int, default=300)
    parser.add_argument("--seed", type=int, default=0)
    args = parser.parse_args()
    for threads in args.threads:
        measure(args.envs, threads, args.steps, args.seed)


if __name__ == "__main__":
    main()
