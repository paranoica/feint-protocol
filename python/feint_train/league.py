from dataclasses import dataclass, field
from pathlib import Path

import numpy as np

HAND_MADE = ("level:3", "level:4")
READERS = ("reader", "reader:anticipating")


def match_value(mine: int, theirs: int, finished: bool, ahead: float = 0.75) -> float:
    if finished:
        return 1.0 if mine > theirs else 0.0
    if mine > theirs:
        return ahead
    if mine < theirs:
        return 1.0 - ahead
    return 0.5


@dataclass
class Budget:
    hand_made: float = 0.2
    level5: float = 0.1
    readers: float = 0.1
    frozen: float = 0.3

    def __post_init__(self) -> None:
        shares = (self.hand_made, self.level5, self.readers, self.frozen)
        if not all(share >= 0.0 for share in shares):
            raise ValueError(f"negative share in {self}")
        if self.level5 > self.hand_made:
            raise ValueError(f"level 5 is part of the hand-made share: {self}")
        if self.hand_made + self.readers + self.frozen > 1.0 + 1e-9:
            raise ValueError(f"shares take more than every seat: {self}")


@dataclass
class Snapshot:
    id: str
    path: Path
    blob: bytes
    value: float = 0.0
    plays: int = 0

    @property
    def rate(self) -> float:
        return (self.value + 1.0) / (self.plays + 2.0)


@dataclass
class Table:
    seats: list[tuple[str, str]]
    opponents: list[str]
    frozen: list[bytes]


@dataclass
class League:
    archive_size: int = 12
    budget: Budget = field(default_factory=Budget)
    archive: list[Snapshot] = field(default_factory=list)

    def add(self, snapshot: str, path: Path) -> None:
        self.archive.append(Snapshot(snapshot, Path(path), Path(path).read_bytes()))
        del self.archive[: max(0, len(self.archive) - self.archive_size)]

    def ids(self) -> list[str]:
        return [s.id for s in self.archive]

    def find(self, snapshot: str) -> Snapshot | None:
        return next((s for s in self.archive if s.id == snapshot), None)

    def rate(self, snapshot: str) -> float:
        found = self.find(snapshot)
        if found is None:
            raise KeyError(snapshot)
        return found.rate

    def record(self, snapshot: str, value: float) -> None:
        found = self.find(snapshot)
        if found is not None:
            found.value += value
            found.plays += 1

    def weights(self) -> np.ndarray:
        return np.array([(1.0 - s.rate) ** 2 for s in self.archive])

    def opponents(self, envs: int, rng: np.random.Generator) -> list[str]:
        b = self.budget
        level5 = int(envs * b.level5)
        others = max(0, int(envs * b.hand_made) - level5)
        out = ["level:5"] * level5
        out += [HAND_MADE[i % len(HAND_MADE)] for i in range(others)]
        out += [READERS[i % len(READERS)] for i in range(int(envs * b.readers))]
        if self.archive:
            weights = self.weights()
            picks = rng.choice(
                len(self.archive), size=int(envs * b.frozen), p=weights / weights.sum()
            )
            out += [self.archive[i].id for i in picks]
        out += ["learner"] * (envs - len(out))
        return out

    def table(self, envs: int, rng: np.random.Generator) -> Table:
        names = self.opponents(envs, rng)
        frozen_ids: list[str] = []
        seats: list[tuple[str, str]] = []
        for index, name in enumerate(names):
            seat = name
            if self.find(name) is not None:
                if name not in frozen_ids:
                    frozen_ids.append(name)
                seat = f"frozen:{frozen_ids.index(name)}"
            seats.append(("learner", seat) if index % 2 == 0 else (seat, "learner"))
        blobs = [s.blob for i in frozen_ids if (s := self.find(i)) is not None]
        return Table(seats=seats, opponents=names, frozen=blobs)

    def state(self) -> dict:
        return {
            "archive_size": self.archive_size,
            "budget": vars(self.budget),
            "archive": [
                {"id": s.id, "path": str(s.path), "value": s.value, "plays": s.plays}
                for s in self.archive
            ],
        }

    @classmethod
    def from_state(cls, state: dict) -> "League":
        league = cls(archive_size=state["archive_size"], budget=Budget(**state["budget"]))
        for item in state["archive"]:
            league.add(item["id"], Path(item["path"]))
            league.archive[-1].value = item["value"]
            league.archive[-1].plays = item["plays"]
        return league
