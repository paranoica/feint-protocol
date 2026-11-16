from pathlib import Path

import numpy as np
import pytest
import torch
from safetensors.numpy import load_file

from feint_train.agent import Agent
from feint_train.export import export, metadata
from feint_train.update import UpdateConfig, ppo_update

CLOSE = {"rel": 1e-5, "abs": 1e-6}

EXPORT = {
    "layer0.bias": ((256,), -2.08607671642676, 5.002997365809385),
    "layer0.weight": ((256, 18), 4.38263231550809, 85.02969619260185),
    "layer1.bias": ((256,), 0.8893688842654228, 0.3339396254919012),
    "layer1.weight": ((256, 256), -1.9001264944672585, 85.15954289143087),
    "layer2.bias": ((2,), 0.0, 0.0),
    "layer2.weight": ((2, 256), 0.00938946773132443, 0.00020000001068560413),
}


def test_hockey_export_is_unchanged(tmp_path: Path) -> None:
    torch.manual_seed(11)
    agent = Agent(np.linspace(0.5, 4.0, 18).astype(np.float32))
    path = tmp_path / "policy.safetensors"
    export(agent, path, level="5")
    tensors = load_file(str(path))
    assert sorted(tensors) == sorted(EXPORT)
    for key, (shape, total, squares) in EXPORT.items():
        value = tensors[key].astype(np.float64)
        assert value.shape == shape, key
        assert value.sum() == pytest.approx(total, **CLOSE), key
        assert (value * value).sum() == pytest.approx(squares, **CLOSE), key
    meta = metadata(path)
    assert meta["format"] == "feint-mlp-v1"
    assert meta["hidden"] == "256,256"
    assert meta["level"] == "5"
    assert meta["features"] == "18"
    assert meta["layout"] == "airhockey-v3:anticipated-observation16+reaction_s+physique"


def test_gaussian_update_is_unchanged() -> None:
    torch.manual_seed(11)
    agent = Agent(np.linspace(0.5, 4.0, 18).astype(np.float32))
    rng = np.random.default_rng(7)
    data = {
        "obs": rng.uniform(-3, 3, (64, 18)).astype(np.float32),
        "action": rng.normal(0, 0.3, (64, 2)).astype(np.float32),
    }
    with torch.no_grad():
        dist = agent.distribution(torch.as_tensor(data["obs"]))
        data["logp"] = dist.log_prob(torch.as_tensor(data["action"])).sum(-1).numpy()
    advantage = rng.normal(0, 1, 64)
    returns = rng.normal(0, 1, 64)
    optimizer = torch.optim.Adam(agent.parameters(), lr=3e-4)
    stats = ppo_update(
        agent,
        optimizer,
        data,
        advantage,
        returns,
        UpdateConfig(epochs=2, minibatches=2),
        np.random.default_rng(1),
    )
    assert stats.policy_loss == pytest.approx(-0.007311120163649321, **CLOSE)
    assert stats.value_loss == pytest.approx(0.6482504457235336, **CLOSE)
    assert stats.entropy == pytest.approx(0.4293280243873596, **CLOSE)
    assert stats.approx_kl == pytest.approx(0.0015546525828540325, rel=1e-4)
    assert stats.clip_fraction == 0.0078125
