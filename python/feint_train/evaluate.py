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
    ahead_at_cap: int = 0
    behind_at_cap: int = 0
    goals_for: int = 0
    goals_against: int = 0

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
    reaction_ms: int = 150,
) -> Result:
    seats = seats_against(opponent, matches)
    learner = np.array([0 if a == "learner" else 1 for a, _ in seats])
    batch = HockeyBatch(
        seats,
        seed=seed,
        cap_ticks=cap_ticks,
        threads=threads,
        reaction_ms=(reaction_ms, reaction_ms),
        physique=(1.0, 1.0),
        goals_to_win=goals_to_win,
    )
    out = batch.reset()
    outcome = np.zeros(matches, dtype=np.int8)
    lead = np.zeros(matches, dtype=np.int64)
    goals = np.zeros(2, dtype=np.int64)
    while (outcome == 0).any():
        actions = np.zeros((matches, 2, 2), dtype=np.float32)
        ready = out["ready"]
        if ready.any():
            actions[ready] = policy(out["obs"][ready])
        out = batch.step(actions)
        ended = (out["terminated"] | out["truncated"]) & (outcome == 0)
        for env in np.flatnonzero(ended):
            mine = int(out["score"][env, learner[env]])
            theirs = int(out["score"][env, 1 - learner[env]])
            goals += (mine, theirs)
            lead[env] = mine - theirs
            if out["truncated"][env]:
                outcome[env] = 3
            else:
                outcome[env] = 1 if mine > theirs else 2
    return Result(
        opponent=opponent,
        wins=int((outcome == 1).sum()),
        losses=int((outcome == 2).sum()),
        unfinished=int((outcome == 3).sum()),
        ahead_at_cap=int(((outcome == 3) & (lead > 0)).sum()),
        behind_at_cap=int(((outcome == 3) & (lead < 0)).sum()),
        goals_for=int(goals[0]),
        goals_against=int(goals[1]),
    )
