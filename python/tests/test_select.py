import json
import shutil
from pathlib import Path

from feint_train.evaluate import Result
from feint_train.export import metadata
from feint_train.select import best_of, candidates, select

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
        [("policy-00010", "level:5", 3, 9, 1), ("policy-final", "level:5", 2, 8, 2)],
    )
    chosen = select(tmp_path, top=2, matches=2, opponent="level:1", threads=2)
    assert chosen in ("policy-00010", "policy-final")
    meta = metadata(tmp_path / "best.safetensors")
    assert meta["selected_from"] == chosen
    assert meta["selected_matches"] == "2"
    assert int(meta["selected_wins"]) >= 0
    assert meta["format"] == "feint-mlp-v1"
