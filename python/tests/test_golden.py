import hashlib
from pathlib import Path

import numpy as np
import torch
from safetensors.numpy import load_file

from feint_train.agent import Agent
from feint_train.export import export, metadata
from feint_train.update import UpdateConfig, ppo_update


def contents_hash(path: Path) -> str:
    h = hashlib.sha256()
    for key, value in sorted(load_file(str(path)).items()):
        h.update(key.encode())
        h.update(value.tobytes())
    for key, value in sorted(metadata(path).items()):
        h.update(key.encode())
        h.update(value.encode())
    return h.hexdigest()


def test_hockey_export_hash_is_unchanged(tmp_path: Path) -> None:
    torch.manual_seed(11)
    agent = Agent(np.linspace(0.5, 4.0, 18).astype(np.float32))
    path = tmp_path / "policy.safetensors"
    export(agent, path, level="5")
    assert contents_hash(path) == (
        "886ddd01881a34a9da1714267faa65dc699683b4cd80c39c1b0616581c06d75b"
    )


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
    assert stats.policy_loss == -0.007311120163649321
    assert stats.value_loss == 0.6482504457235336
    assert stats.entropy == 0.4293280243873596
    assert stats.approx_kl == 0.0015546525828540325
    assert stats.clip_fraction == 0.0078125
