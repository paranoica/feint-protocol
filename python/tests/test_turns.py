from pathlib import Path

import numpy as np
import pytest
import torch
from feint_gym import TurnBatch

from feint_train.rollout import Collector
from feint_train.update import UpdateConfig, ppo_update


def test_turn_batch_shapes_and_masks() -> None:
    batch = TurnBatch("believe", [("learner", "honest"), ("counter", "learner")])
    assert len(batch) == 2
    assert batch.game == "believe"
    assert batch.features == 31
    assert batch.actions == 276
    assert len(batch.scales) == 31 and min(batch.scales) > 0
    assert batch.seats(1) == ("counter", "learner")
    out = batch.reset([3, 4])
    assert out["obs"].shape == (2, 2, 31) and out["obs"].dtype == np.float32
    assert out["legal"].shape == (2, 2, 276)
    assert out["ready"].shape == (2, 2)
    assert out["reward"].shape == (2, 2)
    assert out["terminated"].shape == (2,)
    ready = out["ready"].astype(bool)
    assert ready.sum() == 2
    assert out["legal"][~ready].sum() == 0
    assert (out["legal"][ready].sum(axis=-1) > 0).all()


def test_turn_batch_refuses_bad_input() -> None:
    with pytest.raises(ValueError, match="unknown game"):
        TurnBatch("chess", [("learner", "learner")])
    with pytest.raises(ValueError, match="unknown seat"):
        TurnBatch("kuhn", [("learner", "honest")])
    batch = TurnBatch("kuhn", [("learner", "learner")])
    batch.reset([0])
    with pytest.raises(ValueError, match="shape"):
        batch.step(np.zeros((1, 3), dtype=np.int32))
    with pytest.raises(ValueError, match="illegal"):
        batch.step(np.array([[5, -1]], dtype=np.int32))


def test_masked_head_never_samples_an_illegal_action() -> None:
    from feint_train.agent import DiscreteAgent

    torch.manual_seed(2)
    agent = DiscreteAgent(np.ones(6, dtype=np.float32), actions=5)
    rng = np.random.default_rng(4)
    obs = rng.normal(size=(2000, 6)).astype(np.float32)
    legal = rng.integers(0, 2, size=(2000, 5)).astype(np.uint8)
    legal[np.arange(2000), rng.integers(0, 5, 2000)] = 1
    action, logp, value = agent.sample(obs, legal)
    assert action.shape == (2000,) and logp.shape == (2000,) and value.shape == (2000,)
    assert legal[np.arange(2000), action].all()
    assert (logp <= 0).all()
    assert len(np.unique(action)) == 5


def test_first_epoch_ratio_is_one_with_a_mask() -> None:
    from feint_train.agent import DiscreteAgent

    torch.manual_seed(3)
    agent = DiscreteAgent(np.full(4, 2.0, dtype=np.float32), actions=3)
    rng = np.random.default_rng(5)
    obs = rng.normal(size=(64, 4)).astype(np.float32)
    legal = np.ones((64, 3), dtype=np.uint8)
    legal[::2, 2] = 0
    action, logp, _ = agent.sample(obs, legal)
    data = {"obs": obs, "action": action.astype(np.float32), "logp": logp, "legal": legal}
    optimizer = torch.optim.Adam(agent.parameters(), lr=1e-3)
    stats = ppo_update(
        agent,
        optimizer,
        data,
        rng.normal(size=64),
        rng.normal(size=64),
        UpdateConfig(epochs=1, minibatches=1),
        np.random.default_rng(1),
    )
    assert stats.approx_kl == 0.0
    assert stats.clip_fraction == 0.0
    assert np.isfinite(stats.entropy) and stats.entropy > 0


def test_alternating_learners_credit_the_terminal_reward_to_the_mover() -> None:
    collector = Collector(slots=2, gamma=0.9, potential_scale=1.0, turns=True)
    obs = np.zeros((2, 3), dtype=np.float32)
    legal = np.ones((2, 2), dtype=np.uint8)
    none = np.zeros(1, dtype=bool)

    def out(ready: list[int], reward: list[float], terminated: bool) -> dict:
        return {
            "reward": np.array([reward], dtype=np.float32),
            "ready": np.array([ready], dtype=np.uint8),
            "obs": obs[None],
            "final_obs": obs[None],
            "terminated": np.array([terminated]),
            "truncated": none,
        }

    acts = np.zeros((2, 1), dtype=np.float32)
    collector.act(np.array([1, 0]), obs, acts, np.zeros(2), np.array([0.5, 0.0]), legal)
    collector.credit(out([0, 1], [0.0, 0.0], False), np.zeros(2), np.zeros(2))
    assert 0 in collector.open, "player 0 waits for player 1, its move is still open"
    collector.act(np.array([0, 1]), obs, acts, np.zeros(2), np.array([0.0, -0.25]), legal)
    collector.credit(out([0, 0], [-1.0, 1.0], True), np.zeros(2), np.zeros(2))
    assert not collector.open
    first, second = collector.transitions
    assert (first.slot, first.reward, first.terminal) == (0, -1.0, True)
    assert (second.slot, second.reward, second.terminal) == (1, 1.0, True)
    advantage, returns = collector.advantages(lam=0.95)
    np.testing.assert_allclose(advantage, [-1.5, 1.25])
    np.testing.assert_allclose(returns, [-1.0, 1.0])
    arrays = collector.arrays()
    assert arrays["legal"].shape == (2, 2)


def test_without_turns_the_collector_still_refuses_an_unready_slot() -> None:
    collector = Collector(slots=2, gamma=0.9, potential_scale=1.0)
    obs = np.zeros((2, 3), dtype=np.float32)
    collector.act(np.array([1, 0]), obs, np.zeros((2, 1)), np.zeros(2), np.zeros(2))
    with pytest.raises(RuntimeError, match="neither decided nor ended"):
        collector.credit(
            {
                "reward": np.zeros((1, 2), dtype=np.float32),
                "ready": np.array([[0, 1]], dtype=np.uint8),
                "obs": obs[None],
                "final_obs": obs[None],
                "terminated": np.array([False]),
                "truncated": np.array([False]),
            },
            np.zeros(2),
            np.zeros(2),
        )


def test_turn_export_round_trips_through_numpy(tmp_path: Path) -> None:
    from feint_train.agent import DiscreteAgent
    from feint_train.export import export_turn, forward, metadata

    torch.manual_seed(6)
    scales = np.array([2.0, 1.0, 1.0, 1.0], dtype=np.float32)
    agent = DiscreteAgent(scales, actions=2)
    for p in agent.actor.parameters():
        torch.nn.init.normal_(p, std=0.3)
    path = tmp_path / "kuhn.safetensors"
    export_turn(agent, path, "kuhn")
    obs = np.random.default_rng(1).uniform(-2, 2, (128, 4)).astype(np.float32)
    with torch.no_grad():
        want = agent.logits(torch.as_tensor(obs)).numpy()
    np.testing.assert_allclose(forward(path, obs), want, rtol=1e-5, atol=1e-5)
    meta = metadata(path)
    assert meta["layout"] == "turn-v1:kuhn:4:2"
    assert meta["features"] == "4" and meta["outputs"] == "2"
    assert meta["game"] == "kuhn"
