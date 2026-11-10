import numpy as np
import pytest

from feint_train.rollout import Collector

FEATURES = 3


def out(
    *,
    reward: list[float],
    ready: list[bool],
    obs: list[float] | None = None,
    terminated: bool = False,
    truncated: bool = False,
    final: list[float] | None = None,
) -> dict[str, np.ndarray]:
    def features(xs: list[float] | None) -> np.ndarray:
        rows = [[x, 0.0, 0.0] for x in (xs or [0.0, 0.0])]
        return np.array([rows], dtype=np.float32)

    return {
        "reward": np.array([reward], dtype=np.float32),
        "ready": np.array([ready]),
        "obs": features(obs),
        "final_obs": features(final),
        "terminated": np.array([terminated]),
        "truncated": np.array([truncated]),
    }


def collector(shaping: float = 0.0, gamma: float = 0.5) -> Collector:
    return Collector(slots=2, gamma=gamma, potential_scale=2.0, shaping=shaping)


def act(c: Collector, ready: list[bool], obs: list[float], values: list[float]) -> None:
    feats = np.array([[x, 0.0, 0.0] for x in obs], dtype=np.float32)
    c.act(
        np.array(ready),
        feats,
        np.zeros((2, 2), dtype=np.float32),
        np.zeros(2, dtype=np.float32),
        np.array(values, dtype=np.float32),
    )


ZERO = np.zeros(2, dtype=np.float32)


def test_rewards_between_decisions_go_to_the_open_transition_and_a_decision_links_it() -> None:
    c = collector()
    act(c, [True, False], [0.0, 0.0], [0.4, 0.0])
    c.credit(out(reward=[1.0, -1.0], ready=[True, False]), np.array([0.8, 0.0]), ZERO)
    [t] = c.transitions
    assert (t.reward, t.linked, t.terminal, t.next_value) == (1.0, True, False, pytest.approx(0.8))
    assert not c.open


def test_a_terminal_closes_with_no_bootstrap() -> None:
    c = collector()
    act(c, [True, True], [0.0, 0.0], [0.1, 0.2])
    c.credit(out(reward=[-1.0, 1.0], ready=[False, False], terminated=True), ZERO, ZERO)
    a, b = c.transitions
    assert (a.reward, a.terminal, a.next_value, a.linked) == (-1.0, True, 0.0, False)
    assert (b.reward, b.terminal) == (1.0, True)


def test_a_truncation_bootstraps_from_the_final_features() -> None:
    c = collector()
    act(c, [True, False], [0.0, 0.0], [0.1, 0.0])
    c.credit(
        out(reward=[0.0, 0.0], ready=[False, False], truncated=True),
        ZERO,
        np.array([0.7, 0.0], dtype=np.float32),
    )
    [t] = c.transitions
    assert (t.terminal, t.linked, t.next_value) == (False, False, pytest.approx(0.7))


def test_rewards_before_a_seats_first_decision_are_dropped() -> None:
    c = collector()
    c.credit(out(reward=[1.0, -1.0], ready=[False, False]), ZERO, ZERO)
    assert c.transitions == [] and not c.open


def test_a_seat_that_decided_must_decide_again_or_end() -> None:
    c = collector()
    act(c, [True, False], [0.0, 0.0], [0.0, 0.0])
    with pytest.raises(RuntimeError, match="neither decided nor ended"):
        c.credit(out(reward=[0.0, 0.0], ready=[False, False]), ZERO, ZERO)


def test_advantages_follow_gae_inside_an_episode_and_stop_at_its_end() -> None:
    gamma, lam = 0.5, 0.5
    c = collector(gamma=gamma)
    act(c, [True, False], [0.0, 0.0], [1.0, 0.0])
    c.credit(out(reward=[0.0, 0.0], ready=[True, False]), np.array([2.0, 0.0]), ZERO)
    act(c, [True, False], [0.0, 0.0], [2.0, 0.0])
    c.credit(out(reward=[1.0, 0.0], ready=[False, False], terminated=True), ZERO, ZERO)
    act(c, [True, False], [0.0, 0.0], [3.0, 0.0])
    c.credit(out(reward=[0.0, 0.0], ready=[True, False]), np.array([4.0, 0.0]), ZERO)
    advantage, returns = c.advantages(lam)
    second = 1.0 - 2.0
    first = (0.0 + gamma * 2.0 - 1.0) + gamma * lam * second
    third = 0.0 + gamma * 4.0 - 3.0
    np.testing.assert_allclose(advantage, [first, second, third])
    np.testing.assert_allclose(returns, [first + 1.0, second + 2.0, third + 3.0])


def test_shaping_is_the_discounted_change_of_puck_progress() -> None:
    gamma = 0.9
    c = collector(shaping=0.5, gamma=gamma)
    act(c, [True, False], [1.0, 0.0], [0.0, 0.0])
    c.credit(out(reward=[0.0, 0.0], ready=[True, False], obs=[2.0, 0.0]), ZERO, ZERO)
    act(c, [True, False], [2.0, 0.0], [0.0, 0.0])
    c.credit(out(reward=[1.0, 0.0], ready=[False, False], terminated=True), ZERO, ZERO)
    first, last = c.transitions
    phi = lambda x: 0.5 * x / 2.0  # noqa: E731
    assert first.reward == pytest.approx(gamma * phi(2.0) - phi(1.0))
    assert last.reward == pytest.approx(1.0 - phi(2.0))


def test_open_transitions_block_advantages_and_clearing() -> None:
    c = collector()
    act(c, [True, False], [0.0, 0.0], [0.0, 0.0])
    with pytest.raises(RuntimeError, match="no outcome"):
        c.advantages(0.9)
    with pytest.raises(RuntimeError, match="open transitions"):
        c.clear()
