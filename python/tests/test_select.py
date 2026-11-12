import json
import shutil
from pathlib import Path

from feint_train.evaluate import Result
from feint_train.export import metadata
from feint_train.select import best_of, candidates, fake_candidates, select

CHECKPOINT = Path(__file__).parents[2] / "crates/feint-ml/checkpoints/airhockey-g999.safetensors"


def result(wins: int, goals_for: int, goals_against: int) -> Result:
    return Result(
        opponent="level:5",
        wins=wins,
        losses=0,
        unfinished=200 - wins,
        goals_for=goals_for,
        goals_against=goals_against,
    )


def test_select_picks_the_most_finished_wins_then_goal_difference() -> None:
    scored = {
        "a": result(80, 900, 100),
        "b": result(83, 700, 300),
        "c": result(83, 800, 500),
        "d": result(10, 2000, 0),
    }
    assert best_of(scored) == "b"


def write_evals(logdir: Path, rows: list[tuple[str, str, int, int, int]]) -> None:
    with (logdir / "evals.jsonl").open("w") as f:
        for policy, opponent, wins, goals_for, goals_against in rows:
            line = {
                "policy": policy,
                "opponent": opponent,
                "wins": wins,
                "losses": 0,
                "unfinished": 0,
                "goals_for": goals_for,
                "goals_against": goals_against,
            }
            f.write(json.dumps(line) + "\n")


def test_candidates_are_the_top_policies_against_level_five(tmp_path: Path) -> None:
    write_evals(
        tmp_path,
        [
            ("policy-00010", "level:5", 20, 300, 10),
            ("policy-00010", "level:3", 99, 999, 0),
            ("policy-00020", "level:5", 35, 400, 10),
            ("policy-00030", "level:5", 35, 450, 100),
            ("policy-00040", "level:5", 5, 100, 10),
            ("policy-final", "level:5", 30, 300, 20),
            ("policy-00050", "level:5", 90, 900, 0),
        ],
    )
    for name in ("policy-00010", "policy-00020", "policy-00030", "policy-00040", "policy-final"):
        (tmp_path / f"{name}.safetensors").write_bytes(b"")
    assert candidates(tmp_path, top=3) == ["policy-00020", "policy-00030", "policy-final"]


def test_select_exports_the_winner_with_its_evaluation(tmp_path: Path) -> None:
    for name in ("policy-00010", "policy-final"):
        shutil.copy(CHECKPOINT, tmp_path / f"{name}.safetensors")
    write_evals(
        tmp_path,
        [
            ("policy-00010", "level:1", 3, 9, 1),
            ("policy-final", "level:1", 2, 8, 2),
            ("policy-00020", "level:5", 99, 99, 0),
        ],
    )
    (tmp_path / "policy-00020.safetensors").write_bytes(b"not loaded unless it is a candidate")
    chosen = select(tmp_path, top=2, matches=2, opponent="level:1", threads=2)
    assert chosen in ("policy-00010", "policy-final")
    meta = metadata(tmp_path / "best.safetensors")
    assert meta["selected_from"] == chosen
    assert meta["selected_matches"] == "2"
    assert int(meta["selected_wins"]) >= 0
    assert meta["format"] == "feint-mlp-v1"


def test_fake_candidates_need_the_level5_floor_and_rank_by_bites(tmp_path: Path) -> None:
    rows = [
        ("p1", "level:5", 60, 30, 10, 0),
        ("p1", "keeper:700", 10, 30, 60, 40),
        ("p2", "level:5", 50, 40, 10, 0),
        ("p2", "keeper:700", 10, 30, 60, 90),
        ("p3", "level:5", 57, 30, 13, 0),
        ("p3", "keeper:700", 10, 30, 60, 70),
        ("p4", "level:5", 80, 10, 10, 0),
    ]
    with (tmp_path / "evals.jsonl").open("w") as log:
        for policy, opponent, wins, losses, unfinished, bite_goals in rows:
            row = {
                "policy": policy,
                "opponent": opponent,
                "wins": wins,
                "losses": losses,
                "unfinished": unfinished,
                "goals_for": 0,
                "goals_against": 0,
                "keeper_bite_goals": bite_goals,
            }
            log.write(json.dumps(row) + "\n")
    for name in ("p1", "p2", "p3", "p4"):
        (tmp_path / f"{name}.safetensors").write_bytes(b"")
    assert fake_candidates(tmp_path, top=3, floor=0.56) == ["p3", "p1"]
