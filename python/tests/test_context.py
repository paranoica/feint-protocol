from pathlib import Path

import numpy as np
import torch

from feint_train.context import (
    ContextModel,
    count_baseline,
    export_context,
    load_context,
    losses,
    shifted,
)


def tokens(batch: int = 3, seed: int = 0) -> torch.Tensor:
    g = torch.Generator().manual_seed(seed)
    return torch.rand(batch, 10, 20, generator=g)


def test_inputs_are_shifted_so_a_kick_is_predicted_from_the_kicks_before_it() -> None:
    model = ContextModel()
    x = tokens()
    s = shifted(x, model.start)
    assert s.shape == x.shape
    assert torch.equal(s[:, 0], model.start.expand(3, -1))
    assert torch.equal(s[:, 1:], x[:, :-1])


def test_causal_mask_hides_the_future() -> None:
    torch.manual_seed(1)
    model = ContextModel().eval()
    x = tokens()
    with torch.no_grad():
        base = model(x)
        for t in range(10):
            y = x.clone()
            y[:, t:] = torch.rand_like(y[:, t:])
            out = model(y)
            assert torch.allclose(out[:, : t + 1], base[:, : t + 1], atol=0.0), t
            if t < 9:
                assert not torch.allclose(out[:, t + 1 :], base[:, t + 1 :])


def test_losses_count_only_observed_targets() -> None:
    logits = torch.zeros(1, 2, 8)
    logits[0, 0, 0:3] = torch.tensor([2.0, 0.0, 0.0])
    logits[0, 1, 6] = 1.5
    targets = torch.tensor([[[0, -1, -1, -1], [-1, -1, 1, -1]]], dtype=torch.int64)
    total, parts = losses(logits, targets)
    shot = -torch.log_softmax(torch.tensor([2.0, 0.0, 0.0]), 0)[0]
    guess = torch.nn.functional.softplus(torch.tensor(-1.5))
    assert torch.isclose(parts["shot"], shot)
    assert torch.isclose(parts["guess"], guess)
    assert parts["tell"] == 0.0 and parts["follow"] == 0.0
    assert torch.isclose(total, shot + guess)
    none = torch.full((1, 2, 4), -1, dtype=torch.int64)
    total, _ = losses(logits, none)
    assert total == 0.0


def test_the_count_baseline_is_laplace_over_the_kicks_seen_so_far() -> None:
    shots = np.array([[0, -1, 0, -1, 2, -1, 0]], dtype=np.int64)
    p = count_baseline(shots)
    assert np.allclose(p[0, 0], [1 / 3, 1 / 3, 1 / 3])
    assert np.allclose(p[0, 2], [2 / 4, 1 / 4, 1 / 4])
    assert np.allclose(p[0, 4], [3 / 5, 1 / 5, 1 / 5])
    assert np.allclose(p[0, 6], [3 / 6, 1 / 6, 2 / 6])


def test_export_round_trips(tmp_path: Path) -> None:
    torch.manual_seed(2)
    model = ContextModel().eval()
    path = tmp_path / "context.safetensors"
    export_context(model, path)
    back = load_context(path).eval()
    x = tokens(seed=3)
    with torch.no_grad():
        assert torch.equal(model(x), back(x))
