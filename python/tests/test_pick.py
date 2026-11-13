import json
from pathlib import Path

import pytest

from feint_train.pick import ranked


def write(path: Path, rows: list[dict]) -> None:
    path.write_text("".join(json.dumps(r) + "\n" for r in rows))


def test_candidates_rank_by_the_worst_opponent_share(tmp_path: Path) -> None:
    rows = [
        {"update": 50, "opponent": "a", "mean_reward": 0.4},
        {"update": 50, "opponent": "b", "mean_reward": -0.2},
        {"update": 50, "stats": {"policy_loss": 0.0}},
        {"update": 100, "opponent": "a", "mean_reward": 0.1},
        {"update": 100, "opponent": "b", "mean_reward": 0.0},
        {"update": 150, "opponent": "a", "mean_reward": 0.9},
    ]
    write(tmp_path / "evals.jsonl", rows)
    for update in (50, 100, 150):
        (tmp_path / f"policy-{update:05d}.safetensors").write_bytes(b"x")
    got = ranked(tmp_path, ("a", "b"))
    assert [u for u, _, _ in got] == [100, 50]
    assert got[0][1] == pytest.approx(0.5)
    assert got[1][1] == pytest.approx(0.4)
    assert got[0][2] == tmp_path / "policy-00100.safetensors"


def test_an_update_without_its_export_is_skipped(tmp_path: Path) -> None:
    rows = [
        {"update": 50, "opponent": "a", "mean_reward": 0.4},
        {"update": 100, "opponent": "a", "mean_reward": 0.8},
    ]
    write(tmp_path / "evals.jsonl", rows)
    (tmp_path / "policy-00050.safetensors").write_bytes(b"x")
    assert [u for u, _, _ in ranked(tmp_path, ("a",))] == [50]
