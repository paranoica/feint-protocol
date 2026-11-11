from pathlib import Path

import numpy as np
import torch

from feint_train.agent import Agent
from feint_train.export import FORMAT, LAYOUT, export, forward, metadata


def test_an_exported_policy_computes_what_the_agent_computes(tmp_path: Path) -> None:
    torch.manual_seed(3)
    scales = np.linspace(0.5, 4.0, 18).astype(np.float32)
    agent = Agent(scales)
    for p in agent.actor.parameters():
        torch.nn.init.normal_(p, std=0.3)
    path = tmp_path / "policy.safetensors"
    export(agent, path, level="5")
    obs = np.random.default_rng(3).uniform(-4, 4, (256, 18)).astype(np.float32)
    np.testing.assert_allclose(forward(path, obs), agent.act(obs), rtol=1e-5, atol=1e-4)
    assert metadata(path) == {
        "format": FORMAT,
        "layout": LAYOUT,
        "features": "18",
        "outputs": "2",
        "hidden": "256,256",
        "activation": "relu",
        "level": "5",
        "inv_scales": ",".join(
            np.format_float_positional(x, unique=True) for x in np.float32(1.0) / scales
        ),
    }


def test_the_file_holds_only_little_endian_float32_actor_weights(tmp_path: Path) -> None:
    path = tmp_path / "policy.safetensors"
    export(Agent(np.ones(18, dtype=np.float32)), path)
    raw = path.read_bytes()
    header_len = int.from_bytes(raw[:8], "little")
    header = raw[8 : 8 + header_len].decode()
    assert '"dtype":"F32"' in header.replace(" ", "")
    assert "critic" not in header and "log_std" not in header
    assert '"scales"' not in header
    assert (8 + header_len) % 8 == 0


def test_the_inverse_scales_in_the_metadata_read_back_as_the_exact_float32_values(
    tmp_path: Path,
) -> None:
    scales = np.array([1.0, 0.5, 4.5, 7.0, 3.0, 0.3], dtype=np.float32)
    path = tmp_path / "policy.safetensors"
    export(Agent(scales), path)
    read = np.array(metadata(path)["inv_scales"].split(","), dtype=np.float32)
    np.testing.assert_array_equal(read, np.float32(1.0) / scales)


def test_the_agent_normalises_by_multiplying_with_the_inverse_scales() -> None:
    scales = np.linspace(0.5, 4.0, 18).astype(np.float32)
    agent = Agent(scales)
    x = torch.as_tensor(np.random.default_rng(5).uniform(-4, 4, (512, 18)).astype(np.float32))
    inverse = torch.as_tensor(np.float32(1.0) / scales)
    assert torch.equal(agent.normalise(x), x * inverse)
