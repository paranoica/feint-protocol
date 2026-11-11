from pathlib import Path

import numpy as np
import pytest

from feint_train.league import League, match_value

CHECKPOINT = Path(__file__).parents[2] / "crates/feint-ml/checkpoints/airhockey-g999.safetensors"


def league_with(ids: list[str], size: int = 12) -> League:
    league = League(archive_size=size)
    for snapshot in ids:
        league.add(snapshot, CHECKPOINT)
    return league


def test_match_values_follow_the_result_and_the_lead_at_the_cap() -> None:
    assert match_value(7, 3, finished=True) == 1.0
    assert match_value(2, 7, finished=True) == 0.0
    assert match_value(4, 1, finished=False) == 0.75
    assert match_value(1, 4, finished=False) == 0.25
    assert match_value(2, 2, finished=False) == 0.5


def test_a_fresh_snapshot_starts_at_one_half() -> None:
    league = league_with(["snap-1"])
    assert league.rate("snap-1") == 0.5
    assert league.weights() == pytest.approx([0.25])


def test_league_samples_by_win_rate() -> None:
    league = league_with(["strong", "weak"])
    for _ in range(8):
        league.record("strong", 1.0)
    for _ in range(4):
        league.record("weak", 1.0)
        league.record("weak", 0.0)
    assert league.rate("strong") == pytest.approx(0.9)
    assert league.rate("weak") == pytest.approx(0.5)
    rng = np.random.default_rng(3)
    counts = {"strong": 0, "weak": 0}
    while sum(counts.values()) < 20_000:
        for opponent in league.table(256, rng).opponents:
            if opponent in counts:
                counts[opponent] += 1
    total = sum(counts.values())
    expected = 0.1**2 / (0.1**2 + 0.5**2)
    sigma = (total * expected * (1 - expected)) ** 0.5
    assert abs(counts["strong"] - total * expected) < 4 * sigma


@pytest.mark.parametrize("envs", [256, 16])
def test_league_respects_the_cost_budget(envs: int) -> None:
    league = league_with([f"snap-{i}" for i in range(5)])
    table = league.table(envs, np.random.default_rng(1))
    assert len(table.seats) == envs == len(table.opponents)
    hand_made = [o for o in table.opponents if o.startswith("level:")]
    assert len(hand_made) <= 0.2 * envs
    assert table.opponents.count("level:5") <= 0.1 * envs
    assert table.opponents.count("level:5") >= 1
    for index, (first, second) in enumerate(table.seats):
        assert "learner" in (first, second)
        if table.opponents[index] != "learner":
            assert (first == "learner") == (index % 2 == 0)
    frozen = [s for pair in table.seats for s in pair if s.startswith("frozen:")]
    assert frozen
    assert all(int(s.split(":")[1]) < len(table.frozen) for s in frozen)
    assert all(blob == CHECKPOINT.read_bytes() for blob in table.frozen)


def test_an_empty_archive_gives_no_frozen_seats() -> None:
    table = League().table(32, np.random.default_rng(1))
    assert table.frozen == []
    assert not any(s.startswith("frozen:") for pair in table.seats for s in pair)


def test_archive_keeps_the_newest_and_caps_its_size() -> None:
    league = league_with(["a", "b", "c", "d"], size=3)
    assert league.ids() == ["b", "c", "d"]


def test_tallies_follow_the_snapshot_id_when_the_archive_shifts() -> None:
    league = league_with(["a", "b"], size=2)
    league.record("b", 1.0)
    league.add("c", CHECKPOINT)
    assert league.ids() == ["b", "c"]
    league.record("b", 1.0)
    league.record("a", 0.0)
    assert league.rate("b") == pytest.approx(3 / 4)
    assert league.rate("c") == 0.5


def test_capped_matches_count_by_lead_and_feed_the_league() -> None:
    league = league_with(["ahead", "behind"])
    for _ in range(20):
        league.record("ahead", match_value(3, 1, finished=False))
        league.record("behind", match_value(1, 3, finished=False))
    weights = dict(zip(league.ids(), league.weights(), strict=True))
    assert weights["behind"] > weights["ahead"]


def test_league_state_round_trips() -> None:
    league = league_with(["a", "b"])
    league.record("a", 0.75)
    again = League.from_state(league.state())
    assert again.ids() == league.ids()
    assert again.rate("a") == league.rate("a")
    assert again.archive_size == league.archive_size
