import argparse
import hashlib
import sqlite3
from contextlib import closing
from dataclasses import dataclass, field
from pathlib import Path

import numpy as np
import torch
from feint_gym import HockeyBatch, imitation, pilot_match

from feint_train.agent import Agent
from feint_train.evaluate import evaluate
from feint_train.export import export, loaded

OWN_TARGET = slice(8, 10)
CHANGED = (
    "the database, its WAL or a record changed while reading; "
    "a running arena writes there, stop it before cloning"
)


@dataclass(frozen=True)
class Stored:
    id: int
    path: Path


@dataclass
class Collected:
    features: np.ndarray
    targets: np.ndarray
    groups: np.ndarray
    used: list[tuple[int, int]] = field(default_factory=list)
    refused: list[tuple[int, str]] = field(default_factory=list)


@dataclass
class Fitted:
    agent: Agent
    validation: float
    training: float
    epochs: int


def hockey_records(db: Path) -> list[Stored]:
    path = db.resolve()
    root = path.parent.parent
    if not path.is_file():
        raise sqlite3.OperationalError(f"no database at {path}")
    wal = path.with_name(path.name + "-wal").exists()
    uri = path.as_uri() + ("?mode=ro" if wal else "?immutable=1")
    with closing(sqlite3.connect(uri, uri=True)) as conn:
        rows = conn.execute(
            "SELECT id, record_path FROM matches "
            "WHERE game = 'airhockey' AND finished = 1 AND record_path IS NOT NULL "
            "ORDER BY id"
        ).fetchall()
    return [Stored(int(i), root / str(p)) for i, p in rows]


def collect(stored: list[Stored], reaction_ms: int) -> Collected:
    features, targets, groups = [], [], []
    out = Collected(
        np.zeros((0, HockeyBatch.features), np.float32),
        np.zeros((0, HockeyBatch.outputs), np.float32),
        np.zeros(0, np.int64),
    )
    for s in stored:
        if not s.path.is_file():
            out.refused.append((s.id, "missing"))
            continue
        try:
            got = imitation(s.path.read_bytes(), reaction_ms=reaction_ms)
        except ValueError as error:
            out.refused.append((s.id, str(error)))
            continue
        except OSError as error:
            out.refused.append((s.id, type(error).__name__))
            continue
        rows = len(got["targets"])
        features.append(got["features"])
        targets.append(got["targets"])
        groups.append(np.full(rows, s.id, dtype=np.int64))
        out.used.append((s.id, rows))
    if features:
        out.features = np.concatenate(features)
        out.targets = np.concatenate(targets)
        out.groups = np.concatenate(groups)
    return out


def split_by_match(groups: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    matches = np.unique(groups)
    if len(matches) < 2:
        raise ValueError("behavioural cloning needs at least two matches to hold one out")
    val = groups == matches[-1]
    return ~val, val


def baselines(
    train_targets: np.ndarray, val_features: np.ndarray, val_targets: np.ndarray
) -> tuple[float, float]:
    scales = np.array(HockeyBatch.scales, dtype=np.float32)[OWN_TARGET]
    mean = float(np.mean((val_targets - train_targets.mean(axis=0)) ** 2))
    hold = np.clip(val_features[:, OWN_TARGET] / scales, -1.0, 1.0)
    return mean, float(np.mean((val_targets - hold) ** 2))


def fit(
    features: np.ndarray,
    targets: np.ndarray,
    train: np.ndarray,
    val: np.ndarray,
    seed: int,
    epochs: int = 400,
    batch: int = 256,
    lr: float = 1e-3,
    patience: int = 30,
) -> Fitted:
    torch.manual_seed(seed)
    rng = np.random.default_rng(seed)
    agent = Agent(np.array(HockeyBatch.scales, dtype=np.float32))
    optimiser = torch.optim.Adam(agent.actor.parameters(), lr=lr)
    x = torch.as_tensor(features[train])
    y = torch.as_tensor(targets[train])
    vx = torch.as_tensor(features[val])
    vy = torch.as_tensor(targets[val])

    def loss(a: torch.Tensor, b: torch.Tensor) -> float:
        with torch.no_grad():
            return float(torch.mean((agent.mean(a) - b) ** 2))

    best = (loss(vx, vy), {k: v.clone() for k, v in agent.state_dict().items()}, 0)
    for epoch in range(1, epochs + 1):
        order = torch.as_tensor(rng.permutation(len(x)))
        for start in range(0, len(x), batch):
            pick = order[start : start + batch]
            optimiser.zero_grad()
            torch.mean((agent.mean(x[pick]) - y[pick]) ** 2).backward()
            optimiser.step()
        now = loss(vx, vy)
        if now < best[0]:
            best = (now, {k: v.clone() for k, v in agent.state_dict().items()}, epoch)
        elif epoch - best[2] >= patience:
            break
    agent.load_state_dict(best[1])
    return Fitted(agent, best[0], loss(x, y), best[2])


def snapshot(db: Path, records: list[Path]) -> dict[str, str | None]:
    wal = db.with_name(db.name + "-wal")
    shm = db.with_name(db.name + "-shm")
    seen: dict[str, str | None] = {str(shm): "present" if shm.exists() else None}
    for path in [db, wal, *records]:
        seen[str(path)] = hashlib.sha256(path.read_bytes()).hexdigest() if path.is_file() else None
    return seen


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--db", type=Path, default=Path("../data/feint.db"))
    parser.add_argument("--out", type=Path, default=Path("runs/bc/human.safetensors"))
    parser.add_argument("--reaction-ms", type=int, default=300)
    parser.add_argument("--matches", type=int, default=20)
    parser.add_argument("--seed", type=int, default=40000)
    parser.add_argument("--threads", type=int, default=6)
    args = parser.parse_args()

    before = snapshot(args.db, [])
    stored = hockey_records(args.db)
    paths = [s.path for s in stored]
    if snapshot(args.db, []) != before:
        raise SystemExit(CHANGED)
    before = snapshot(args.db, paths)
    data = collect(stored, args.reaction_ms)
    for i, rows in data.used:
        print(f"match {i}: {rows} decisions")
    for i, reason in data.refused:
        print(f"match {i}: refused ({reason})")
    print(
        f"matches used {len(data.used)}, refused {len(data.refused)}, decisions {len(data.targets)}"
    )
    if snapshot(args.db, paths) != before:
        raise SystemExit(CHANGED)
    try:
        train, val = split_by_match(data.groups)
    except ValueError as error:
        raise SystemExit(
            f"{error}; play more matches on the arena at the current version"
        ) from None
    fitted = fit(data.features, data.targets, train, val, seed=args.seed)
    mean, hold = baselines(data.targets[train], data.features[val], data.targets[val])
    held = int(data.groups[val][0])
    print(
        f"held-out match {held} ({int(val.sum())} decisions, also used for early stopping, "
        f"so optimistic): loss {fitted.validation:.4f}, "
        f"predict-the-mean {mean:.4f}, hold-the-target {hold:.4f}; "
        f"training loss {fitted.training:.4f}, best epoch {fitted.epochs}"
    )
    export(fitted.agent, args.out, level="bc")
    print(f"wrote {args.out}")

    one = pilot_match(args.out.read_bytes(), level=1, seed=args.seed, reaction_ms=args.reaction_ms)
    print(
        f"rust pilot vs hand-made level 1: score {one['score'][0]}:{one['score'][1]}, "
        f"ticks {one['ticks']}, finished {one['finished']}"
    )
    policy = loaded(args.out)
    for opponent in ("human", "level:1"):
        r = evaluate(
            policy,
            opponent,
            args.matches,
            seed=args.seed + 1,
            threads=args.threads,
            reaction_ms=args.reaction_ms,
        )
        print(
            f"vs {opponent}: won {r.wins}, lost {r.losses}, unfinished {r.unfinished}, "
            f"goals {r.goals_for}:{r.goals_against}"
        )

    after = snapshot(args.db, paths)
    if after != before:
        raise SystemExit(CHANGED)
    print(
        f"unchanged: the database, its WAL and {len(paths)} records hashed before reading "
        "and after; the -shm index only for being there (a reader writes its read marks in it)"
    )


if __name__ == "__main__":
    main()
