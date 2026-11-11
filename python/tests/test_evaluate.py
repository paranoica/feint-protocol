import numpy as np

from feint_train.evaluate import evaluate, seats_against


def still(obs: np.ndarray) -> np.ndarray:
    return np.tile(np.array([-0.95, 0.95], dtype=np.float32), (len(obs), 1))


def test_seats_alternate_so_the_learner_plays_both_ends() -> None:
    assert seats_against("level:2", 3) == [
        ("learner", "level:2"),
        ("level:2", "learner"),
        ("learner", "level:2"),
    ]


def test_a_learner_that_never_moves_loses_every_match_to_level_five() -> None:
    result = evaluate(still, "level:5", 6, seed=1, threads=2, goals_to_win=1)
    assert (result.wins, result.losses, result.unfinished) == (0, 6, 0)
    assert result.share == 0.0


def test_matches_cut_by_the_cap_are_unfinished_not_lost() -> None:
    result = evaluate(still, "reader", 4, seed=1, threads=2, cap_ticks=400)
    assert (result.wins, result.losses, result.unfinished) == (0, 0, 4)


def test_a_still_learner_cut_by_the_cap_is_behind_from_either_end() -> None:
    result = evaluate(still, "level:5", 4, seed=3, threads=2, cap_ticks=240 * 30, goals_to_win=50)
    assert (result.wins, result.losses, result.unfinished) == (0, 0, 4)
    assert (result.ahead_at_cap, result.behind_at_cap) == (0, 4)
    assert result.goals_for == 0
    assert result.goals_against >= 4


def test_a_goalless_cap_is_neither_ahead_nor_behind() -> None:
    result = evaluate(still, "reader", 4, seed=1, threads=2, cap_ticks=72)
    assert (result.ahead_at_cap, result.behind_at_cap) == (0, 0)
    assert (result.goals_for, result.goals_against) == (0, 0)


def test_finished_matches_count_their_goals() -> None:
    result = evaluate(still, "level:5", 6, seed=1, threads=2, goals_to_win=1)
    assert (result.goals_for, result.goals_against) == (0, 6)
    assert (result.ahead_at_cap, result.behind_at_cap) == (0, 0)
