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


def test_kuhn_learner_beats_always_bet(tmp_path: Path) -> None:
    from feint_train.turns import TurnConfig, evaluate, train

    config = TurnConfig(
        game="kuhn",
        envs=256,
        updates=60,
        mix={"bet": 1.0},
        evaluate=("bet",),
        eval_every=1000,
        eval_matches=2000,
        seed=3,
        logdir=tmp_path,
        lr=3e-3,
        entropy=0.01,
    )
    agent = train(config)
    result = evaluate(agent, "kuhn", "bet", matches=4000, seed=11)
    assert result["mean_reward"] > 0.25, result
    assert (tmp_path / "policy-00060.safetensors").exists()
    assert (tmp_path / "checkpoint.pt").exists()


def test_seat_plan_alternates_the_learner_and_follows_the_mix() -> None:
    from feint_train.turns import seat_plan

    rng = np.random.default_rng(0)
    seats, learner = seat_plan(1000, {"learner": 0.5, "honest": 0.3, "frozen": 0.2}, 4, rng)
    assert len(seats) == 1000
    assert learner.tolist()[:4] == [0, 1, 0, 1]
    for (a, b), side in zip(seats, learner, strict=True):
        assert (a, b)[side] == "learner"
    others = [pair[1 - side] for pair, side in zip(seats, learner, strict=True)]
    assert 420 < sum(o == "learner" for o in others) < 580
    assert 230 < sum(o == "honest" for o in others) < 370
    frozen = [o for o in others if o.startswith("frozen:")]
    assert 140 < len(frozen) < 260
    assert {int(o.split(":")[1]) for o in frozen} <= {0, 1, 2, 3}
    seats, _ = seat_plan(10, {"learner": 0.5, "frozen": 0.5}, 0, rng)
    assert all(not s.startswith("frozen") for pair in seats for s in pair)


def test_play_records_only_learner_moves_and_ends_every_match() -> None:
    from feint_train.agent import DiscreteAgent
    from feint_train.turns import play, slot_masks

    torch.manual_seed(1)
    probe = TurnBatch("kuhn", [("learner", "learner")])
    agent = DiscreteAgent(np.array(probe.scales, dtype=np.float32), probe.actions)
    frozen = DiscreteAgent(np.array(probe.scales, dtype=np.float32), probe.actions)
    seats = [("learner", "learner"), ("frozen:0", "learner"), ("learner", "random")] * 40
    learner = np.array([0, 1, 0] * 40)
    masks = slot_masks(seats, learner)
    collector = Collector(slots=2 * len(seats), gamma=1.0, potential_scale=1.0, turns=True)
    total = play(
        agent,
        TurnBatch("kuhn", seats),
        list(range(len(seats))),
        masks,
        {"frozen:0": frozen},
        collector,
    )
    assert not collector.open
    slots = {t.slot for t in collector.transitions}
    frozen_slots = set(np.flatnonzero(masks["frozen:0"]).tolist())
    scripted_slots = {2 * env + 1 for env in range(2, len(seats), 3)}
    assert slots.isdisjoint(frozen_slots | scripted_slots)
    assert slots == set(np.flatnonzero(masks["learner"]).tolist())
    assert all(t.closed for t in collector.transitions)
    pairs = total.reshape(-1, 2)
    assert np.all(pairs.sum(axis=1) == 0), "Kuhn is zero-sum"
    assert np.all(np.abs(pairs) >= 1)


class EndsAtReset:
    def __len__(self) -> int:
        return 1

    def reset(self, seeds: list[int]) -> dict:
        return {
            "obs": np.zeros((1, 2, 4), dtype=np.float32),
            "legal": np.zeros((1, 2, 2), dtype=np.uint8),
            "ready": np.zeros((1, 2), dtype=np.uint8),
            "reward": np.array([[1.0, -1.0]], dtype=np.float32),
            "terminated": np.array([1], dtype=np.uint8),
            "score": np.array([[1, -1]]),
        }

    def step(self, actions: np.ndarray) -> dict:
        raise AssertionError("a finished match was stepped")


def test_a_match_that_ends_at_reset_is_not_waited_for() -> None:
    from feint_train.agent import DiscreteAgent
    from feint_train.turns import play

    agent = DiscreteAgent(np.ones(4, dtype=np.float32), 2)
    masks = {"learner": np.array([True, False])}
    total = play(agent, EndsAtReset(), [0], masks, {}, None)
    np.testing.assert_array_equal(total, [1.0, -1.0])


def test_a_time_budget_still_saves_the_last_policy(tmp_path: Path) -> None:
    from feint_train.turns import TurnConfig, train

    config = TurnConfig(
        game="kuhn", logdir=tmp_path, envs=16, updates=50, hours=1e-7, eval_every=25, seed=2
    )
    train(config)
    saved = torch.load(tmp_path / "checkpoint.pt", weights_only=True)
    assert saved["update"] >= 1
    assert (tmp_path / f"policy-{saved['update']:05d}.safetensors").exists()


def test_a_resumed_run_ends_where_an_unbroken_run_ends(tmp_path: Path) -> None:
    from safetensors.numpy import load_file

    from feint_train.turns import TurnConfig, train

    def config(logdir: Path, updates: int, resume: bool) -> TurnConfig:
        return TurnConfig(
            game="kuhn",
            logdir=logdir,
            envs=32,
            updates=updates,
            eval_every=2,
            eval_matches=50,
            evaluate=("random",),
            mix={"learner": 0.5, "frozen": 0.5},
            snapshot_every=1,
            seed=4,
            resume=resume,
        )

    train(config(tmp_path / "whole", 4, False))
    train(config(tmp_path / "split", 2, False))
    train(config(tmp_path / "split", 4, True))
    whole = load_file(str(tmp_path / "whole" / "policy-00004.safetensors"))
    split = load_file(str(tmp_path / "split" / "policy-00004.safetensors"))
    for key, value in whole.items():
        np.testing.assert_array_equal(value, split[key], err_msg=key)


def test_a_policy_with_non_finite_weights_is_not_exported(tmp_path: Path) -> None:
    from feint_train.agent import DiscreteAgent
    from feint_train.export import export_turn

    agent = DiscreteAgent(np.ones(4, dtype=np.float32), 2)
    with torch.no_grad():
        agent.actor[4].bias[0] = float("nan")
    with pytest.raises(ValueError, match="finite"):
        export_turn(agent, tmp_path / "p.safetensors", "kuhn")
    assert not (tmp_path / "p.safetensors").exists()
