import numpy as np
import pytest
from feint_gym import TurnBatch


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
