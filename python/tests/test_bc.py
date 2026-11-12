import hashlib
import sqlite3
from contextlib import closing
from pathlib import Path

import numpy as np
import pytest
from feint_gym import HockeyBatch, pilot_match

from feint_train import evaluate as evaluate_module
from feint_train.bc import (
    Stored,
    baselines,
    collect,
    fit,
    hockey_records,
    snapshot,
    split_by_match,
)
from feint_train.export import export, metadata

FOCUS = Path(__file__).parents[2] / "crates/feint-ml/checkpoints/airhockey-focus.safetensors"


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def arena_like(db: Path) -> None:
    with closing(sqlite3.connect(db)) as conn:
        conn.execute("PRAGMA journal_mode=WAL")
        conn.execute(
            "CREATE TABLE matches (id INTEGER PRIMARY KEY, game TEXT NOT NULL, "
            "finished INTEGER NOT NULL, record_path TEXT)"
        )
        conn.executemany(
            "INSERT INTO matches VALUES (?, ?, ?, ?)",
            [
                (1, "airhockey", 1, "data/matches/1.rec"),
                (2, "penalty", 1, "data/matches/penalty/2.rec"),
                (3, "airhockey", 0, "data/matches/3.rec"),
                (4, "airhockey", 1, None),
                (5, "airhockey", 1, "data/matches/airhockey/5.rec"),
            ],
        )
        conn.commit()


def test_only_finished_hockey_rows_are_read_and_the_file_is_untouched(tmp_path: Path) -> None:
    db = tmp_path / "data" / "feint.db"
    db.parent.mkdir()
    arena_like(db)
    before = snapshot(db, [])
    assert before[str(db.with_name("feint.db-wal"))] is None
    got = hockey_records(db)
    assert snapshot(db, []) == before
    assert sorted(p.name for p in db.parent.iterdir()) == ["feint.db"]
    assert got == [
        Stored(1, tmp_path / "data/matches/1.rec"),
        Stored(5, tmp_path / "data/matches/airhockey/5.rec"),
    ]


def test_the_snapshot_sees_sidecars_a_plain_read_only_open_creates(tmp_path: Path) -> None:
    db = tmp_path / "feint.db"
    arena_like(db)
    before = snapshot(db, [])
    with closing(sqlite3.connect(db.resolve().as_uri() + "?mode=ro", uri=True)) as conn:
        conn.execute("SELECT count(*) FROM matches").fetchall()
    assert snapshot(db, []) != before


def test_the_snapshot_sees_a_new_shm_next_to_an_existing_wal(tmp_path: Path) -> None:
    source = tmp_path / "live" / "feint.db"
    source.parent.mkdir()
    arena_like(source)
    copy = tmp_path / "copy" / "feint.db"
    copy.parent.mkdir()
    with closing(sqlite3.connect(source)) as writer:
        writer.execute("INSERT INTO matches VALUES (6, 'airhockey', 1, 'data/matches/6.rec')")
        writer.commit()
        for name in ("feint.db", "feint.db-wal"):
            (copy.parent / name).write_bytes((source.parent / name).read_bytes())
    before = snapshot(copy, [])
    with closing(sqlite3.connect(copy.resolve().as_uri() + "?mode=ro", uri=True)) as conn:
        conn.execute("SELECT count(*) FROM matches").fetchall()
    assert (copy.parent / "feint.db-shm").exists()
    assert snapshot(copy, []) != before


def test_rows_still_in_the_wal_are_read(tmp_path: Path) -> None:
    db = tmp_path / "data" / "feint.db"
    db.parent.mkdir()
    arena_like(db)
    with closing(sqlite3.connect(db)) as writer:
        writer.execute("INSERT INTO matches VALUES (6, 'airhockey', 1, 'data/matches/6.rec')")
        writer.commit()
        assert (db.parent / "feint.db-wal").stat().st_size > 0
        before = digest(db), digest(db.parent / "feint.db-wal")
        got = hockey_records(db)
        assert (digest(db), digest(db.parent / "feint.db-wal")) == before
    assert [s.id for s in got] == [1, 5, 6]


def test_a_missing_database_is_an_error_not_a_new_file(tmp_path: Path) -> None:
    db = tmp_path / "data" / "feint.db"
    with pytest.raises(sqlite3.OperationalError):
        hockey_records(db)
    assert not db.exists()


def test_split_keeps_each_match_whole() -> None:
    groups = np.array([1, 1, 1, 2, 2, 3, 3, 3])
    train, val = split_by_match(groups)
    assert val.tolist() == [False] * 5 + [True] * 3
    assert train.tolist() == [True] * 5 + [False] * 3
    with pytest.raises(ValueError, match="two matches"):
        split_by_match(np.array([4, 4, 4]))


def test_refused_records_are_reported_with_their_reason(tmp_path: Path) -> None:
    good = tmp_path / "1.rec"
    good.write_bytes(
        pilot_match(FOCUS.read_bytes(), level=1, seed=1, reaction_ms=150, max_ticks=600)["record"]
    )
    junk = tmp_path / "2.rec"
    junk.write_bytes(b"junk")
    gone = tmp_path / "3.rec"
    locked = tmp_path / "4.rec"
    locked.write_bytes(b"locked")
    locked.chmod(0)
    stored = [Stored(1, good), Stored(2, junk), Stored(3, gone), Stored(4, locked)]
    data = collect(stored, reaction_ms=150)
    decisions = len([t for t in range(600) if t % 4 == 0 and t >= 36])
    assert data.used == [(1, decisions)]
    assert [(i, reason.split("(")[0]) for i, reason in data.refused] == [
        (2, "Decode"),
        (3, "missing"),
        (4, "PermissionError"),
    ]
    assert data.features.shape == (decisions, 18)
    assert data.groups.tolist() == [1] * decisions


def test_hold_baseline_reads_the_mallets_own_target() -> None:
    scales = np.array(HockeyBatch.scales, dtype=np.float32)
    features = np.zeros((2, 18), dtype=np.float32)
    features[:, 8] = [scales[8] * 0.5, -scales[8]]
    features[:, 9] = [0.0, scales[9] * 0.25]
    targets = np.array([[0.5, 0.0], [-1.0, 0.25]], dtype=np.float32)
    mean, hold = baselines(targets, features, targets)
    assert hold == 0.0
    assert mean == pytest.approx(float(np.mean((targets - targets.mean(axis=0)) ** 2)))


def teacher_data(tmp_path: Path, seeds: range) -> list[Stored]:
    stored = []
    for seed in seeds:
        path = tmp_path / f"{seed}.rec"
        played = pilot_match(
            FOCUS.read_bytes(), level=1, seed=seed, reaction_ms=150, max_ticks=240 * 60
        )
        path.write_bytes(played["record"])
        stored.append(Stored(seed, path))
    return stored


def test_clone_of_a_known_policy_beats_both_baselines(tmp_path: Path) -> None:
    data = collect(teacher_data(tmp_path, range(1, 5)), reaction_ms=150)
    train, val = split_by_match(data.groups)
    fitted = fit(data.features, data.targets, train, val, seed=0, epochs=60)
    mean, hold = baselines(data.targets[train], data.features[val], data.targets[val])
    assert fitted.validation < 0.2 * mean, (fitted.validation, mean)
    assert fitted.validation < hold, (fitted.validation, hold)
    path = tmp_path / "bc.safetensors"
    export(fitted.agent, path, level="bc")
    assert metadata(path)["level"] == "bc"
    played = pilot_match(path.read_bytes(), level=1, seed=3, reaction_ms=300, max_ticks=2400)
    assert played["ticks"] == 2400 or played["finished"]


def test_reaction_reaches_the_batch(monkeypatch: pytest.MonkeyPatch) -> None:
    seen: dict[str, object] = {}

    class Stop(Exception):
        pass

    def fake(*args: object, **kwargs: object) -> None:
        seen.update(kwargs)
        raise Stop

    monkeypatch.setattr(evaluate_module, "HockeyBatch", fake)
    with pytest.raises(Stop):
        evaluate_module.evaluate(lambda x: x, "human", 2, seed=1, reaction_ms=300)
    assert seen["reaction_ms"] == (300, 300)
    with pytest.raises(Stop):
        evaluate_module.evaluate(lambda x: x, "human", 2, seed=1)
    assert seen["reaction_ms"] == (150, 150)
