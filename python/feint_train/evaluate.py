from collections.abc import Callable
from dataclasses import dataclass

import numpy as np
from feint_gym import HockeyBatch

EVAL_CAP_TICKS = 240 * 600


@dataclass
class Result:
    opponent: str
    wins: int
    losses: int
    unfinished: int

    @property
    def matches(self) -> int:
        return self.wins + self.losses + self.unfinished

    @property
    def share(self) -> float:
        return self.wins / max(1, self.matches)


def seats_against(opponent: str, matches: int) -> list[tuple[str, str]]:
    return [
        ("learner", opponent) if index % 2 == 0 else (opponent, "learner")
        for index in range(matches)
    ]


def evaluate(
    policy: Callable[[np.ndarray], np.ndarray],
    opponent: str,
    matches: int,
    seed: int,
    threads: int = 6,
    cap_ticks: int = EVAL_CAP_TICKS,
    goals_to_win: int | None = None,
) -> Result:
    seats = seats_against(opponent, matches)
    learner = np.array([0 if a == "learner" else 1 for a, _ in seats])
    batch = HockeyBatch(
        seats,
        seed=seed,
        cap_ticks=cap_ticks,
        threads=threads,
        reaction_ms=(150, 150),
        physique=(1.0, 1.0),
        goals_to_win=goals_to_win,
    )
    out = batch.reset()
    outcome = np.zeros(matches, dtype=np.int8)
    while (outcome == 0).any():
        actions = np.zeros((matches, 2, 2), dtype=np.float32)
        ready = out["ready"]
        if ready.any():
            actions[ready] = policy(out["obs"][ready])
        out = batch.step(actions)
        ended = (out["terminated"] | out["truncated"]) & (outcome == 0)
        for env in np.flatnonzero(ended):
            if out["truncated"][env]:
                outcome[env] = 3
            else:
                mine = out["score"][env, learner[env]]
                theirs = out["score"][env, 1 - learner[env]]
                outcome[env] = 1 if mine > theirs else 2
    return Result(
        opponent=opponent,
        wins=int((outcome == 1).sum()),
        losses=int((outcome == 2).sum()),
        unfinished=int((outcome == 3).sum()),
    )
