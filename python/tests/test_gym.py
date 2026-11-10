import threading

import numpy as np
import pytest
from feint_gym import HockeyBatch

from feint_train.throughput import mix

MIX = [
    ("learner", "learner"),
    ("learner", "level:3"),
    ("human", "learner"),
    ("learner", "reader:anticipating"),
]


def policy(out: dict[str, np.ndarray]) -> np.ndarray:
    obs = out["obs"]
    actions = np.stack([obs[..., 0] / 1.2 - 0.3, obs[..., 1] / 0.6], axis=-1)
    return np.where(out["ready"][..., None], actions, 0.0).astype(np.float32)


def run(seed: int, threads: int, steps: int) -> list[dict[str, np.ndarray]]:
    batch = HockeyBatch(MIX * 2, seed=seed, threads=threads)
    out = batch.reset()
    seen = [out]
    for _ in range(steps):
        out = batch.step(policy(out))
        seen.append(out)
    return seen


def test_reset_gives_arrays_of_the_batch_shape() -> None:
    batch = HockeyBatch(MIX, seed=1)
    out = batch.reset()
    assert len(batch) == 4
    assert HockeyBatch.features == 18 and HockeyBatch.outputs == 2
    expected = {
        "obs": ((4, 2, 18), np.float32),
        "final_obs": ((4, 2, 18), np.float32),
        "ready": ((4, 2), np.bool_),
        "reward": ((4, 2), np.float32),
        "terminated": ((4,), np.bool_),
        "truncated": ((4,), np.bool_),
        "score": ((4, 2), np.int64),
    }
    assert set(out) == set(expected)
    for name, (shape, dtype) in expected.items():
        assert out[name].shape == shape, name
        assert out[name].dtype == dtype, name
    assert out["ready"].any(axis=1).all()
    assert not out["ready"][2, 0] and not out["ready"][1, 1]


def test_same_seed_same_trajectory_whatever_the_threads() -> None:
    one, four, other = run(5, 1, 120), run(5, 4, 120), run(6, 4, 120)
    for a, b in zip(one, four, strict=True):
        for name in a:
            np.testing.assert_array_equal(a[name], b[name], err_msg=name)
    assert any(not np.array_equal(a["obs"], b["obs"]) for a, b in zip(one, other, strict=True))


def test_episode_reports_the_seed_reaction_and_body() -> None:
    batch = HockeyBatch(
        [("learner", "level:2")], seed=3, reaction_ms=(200, 200), physique=(0.7, 0.7)
    )
    out = batch.reset()
    seed, reaction, physique = batch.episode(0)
    assert seed >= 1 << 63
    assert reaction == (200, 300)
    assert physique[0] == 0.7
    assert out["obs"][0, 0, 16] == np.float32(0.2)
    assert out["obs"][0, 0, 17] == np.float32(0.7)
    before = batch.ticks(0)
    batch.step(policy(out))
    assert batch.ticks(0) == before + 4
    assert isinstance(batch.digest(0), int)


def test_a_capped_match_is_truncated_and_starts_again() -> None:
    batch = HockeyBatch([("learner", "reader")], seed=2, cap_ticks=800)
    out = batch.reset()
    for _ in range(400):
        out = batch.step(np.full((1, 2, 2), -0.9, dtype=np.float32))
        if out["truncated"][0]:
            break
    assert out["truncated"][0] and not out["terminated"][0]
    assert np.any(out["final_obs"][0, 0] != 0.0)
    assert batch.ticks(0) < 200


@pytest.mark.parametrize(
    ("seats", "kwargs", "message"),
    [
        ([], {}, "at least one env"),
        ([("human", "reader")], {}, "no learner"),
        ([("learner", "level:9")], {}, "level 9"),
        ([("learner", "goalie")], {}, "unknown seat"),
        ([("learner", "human")], {"threads": 0}, "thread"),
        ([("learner", "human")], {"cap_ticks": 6}, "decision stride"),
        ([("learner", "human")], {"reaction_ms": (400, 400), "cap_ticks": 96}, "at least 100"),
        ([("learner", "human")], {"reaction_ms": (150, 300_000)}, "delay limit"),
        ([("learner", "human")], {"goals_to_win": 0}, "one goal to win"),
        ([("learner", "human")], {"physique": (0.0, 1.0)}, "ranges"),
    ],
)
def test_malformed_batches_are_refused(seats: list, kwargs: dict, message: str) -> None:
    with pytest.raises(ValueError, match=message):
        HockeyBatch(seats, seed=1, **kwargs)


def test_actions_of_the_wrong_shape_are_refused() -> None:
    batch = HockeyBatch(MIX, seed=1)
    batch.reset()
    with pytest.raises(ValueError, match="shape"):
        batch.step(np.zeros((4, 2, 3), dtype=np.float32))
    with pytest.raises(ValueError, match="shape"):
        batch.step(np.zeros((3, 2, 2), dtype=np.float32))
    batch.step(np.zeros((4, 2, 2), dtype=np.float32))


def test_actions_that_are_not_plain_float32_memory_are_refused() -> None:
    batch = HockeyBatch(MIX, seed=1)
    batch.reset()
    record = np.zeros((4, 2, 2), dtype=[("a", "f4"), ("b", "u1")])
    shifted = np.frombuffer(bytes(4 * 2 * 2 * 4 + 1), dtype=np.float32, offset=1).reshape(4, 2, 2)
    fortran = np.asfortranarray(np.zeros((4, 2, 2), dtype=np.float32))
    for bad in (record["a"], shifted, fortran):
        with pytest.raises(ValueError, match="C-contiguous"):
            batch.step(bad)
    with pytest.raises(TypeError):
        batch.step(np.zeros((4, 2, 2), dtype=np.float64))
    batch.step(np.ascontiguousarray(fortran))


def test_goals_to_win_ends_the_match() -> None:
    batch = HockeyBatch([("level:5", "learner")], seed=4, goals_to_win=1)
    out = batch.reset()
    total = np.zeros(2, dtype=np.float32)
    for _ in range(20_000):
        out = batch.step(np.full((1, 2, 2), -0.9, dtype=np.float32))
        total += out["reward"][0]
        if out["terminated"][0]:
            break
    assert out["terminated"][0] and not out["truncated"][0]
    assert out["score"][0].tolist() == [1, 0]
    assert total.tolist() == [1.0, -1.0]


def test_every_seat_name_is_read_as_written() -> None:
    names = ["learner", "human", "reader", "reader:anticipating"] + [
        f"level:{n}" for n in range(1, 6)
    ]
    batch = HockeyBatch([("learner", name) for name in names], seed=1)
    assert [batch.seats(env) for env in range(len(names))] == [("learner", n) for n in names]
    with pytest.raises(ValueError, match="no env"):
        batch.seats(len(names))


def test_the_training_mix_steps_at_full_size() -> None:
    batch = HockeyBatch(mix(256), seed=11, threads=4)
    out = batch.reset()
    decided = out["ready"].copy()
    for _ in range(20):
        out = batch.step(policy(out))
        decided |= out["ready"]
        assert out["obs"].shape == (256, 2, 18)
        assert np.isfinite(out["obs"]).all()
    learners = np.array([[a == "learner", b == "learner"] for a, b in mix(256)])
    assert (decided == learners).all()


def test_another_thread_never_deadlocks_a_step() -> None:
    batch = HockeyBatch(mix(64), seed=2, threads=2)
    out = batch.reset()
    done = threading.Event()
    answers: list[str] = []

    def poke() -> None:
        while not done.is_set():
            try:
                batch.ticks(0)
                answers.append("ticks")
            except RuntimeError:
                answers.append("busy")

    other = threading.Thread(target=poke)
    other.start()
    for _ in range(30):
        out = batch.step(policy(out))
    done.set()
    other.join(timeout=10)
    assert not other.is_alive()
    assert len(batch) == 64
    assert answers
