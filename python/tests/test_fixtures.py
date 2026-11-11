import struct
from pathlib import Path

import numpy as np
import torch
from feint_gym import HockeyBatch

from feint_train.agent import Agent
from feint_train.export import forward
from feint_train.fixtures import extreme_features, make_fixtures, read_matrix, write_matrix

FEATURES = 18


def saved_agent(tmp_path: Path) -> Path:
    torch.manual_seed(11)
    agent = Agent(np.array(HockeyBatch.scales, dtype=np.float32))
    for p in agent.actor.parameters():
        torch.nn.init.normal_(p, std=0.2)
    path = tmp_path / "checkpoint.pt"
    torch.save({"agent": agent.state_dict()}, path)
    return path


def test_a_matrix_file_is_count_width_then_little_endian_float32(tmp_path: Path) -> None:
    data = np.array([[1.0, -0.0], [2.5, 3.0], [-4.0, 5.0]], dtype=np.float32)
    path = tmp_path / "m.bin"
    write_matrix(path, data)
    raw = path.read_bytes()
    assert struct.unpack("<II", raw[:8]) == (3, 2)
    assert struct.unpack("<6f", raw[8:]) == (1.0, -0.0, 2.5, 3.0, -4.0, 5.0)
    assert len(raw) == 8 + 6 * 4
    np.testing.assert_array_equal(read_matrix(path), data)


def test_extremes_hold_zeros_both_signs_of_every_scale_and_saturating_values() -> None:
    scales = np.array(HockeyBatch.scales, dtype=np.float32)
    rows = extreme_features(scales)
    assert rows.shape[1] == FEATURES
    assert any((row == 0).all() for row in rows)
    for sign in (1.0, -1.0):
        assert any(np.array_equal(row, sign * scales) for row in rows)
        assert any(np.array_equal(row, sign * 10 * scales) for row in rows)
    for i in range(FEATURES):
        one = np.zeros(FEATURES, dtype=np.float32)
        one[i] = scales[i]
        assert any(np.array_equal(row, one) for row in rows)
        assert any(np.array_equal(row, -one) for row in rows)


def test_fixtures_are_what_the_exported_policy_computes(tmp_path: Path) -> None:
    checkpoint = saved_agent(tmp_path)
    out = tmp_path / "fixtures"
    policy = tmp_path / "policy.safetensors"
    make_fixtures(checkpoint, out, policy, recorded=24, seed=4242, threads=2)
    inputs, outputs = read_matrix(out / "features.bin"), read_matrix(out / "outputs.bin")
    scales = np.array(HockeyBatch.scales, dtype=np.float32)
    assert inputs.shape == (24 + len(extreme_features(scales)), FEATURES)
    assert outputs.shape == (len(inputs), 2)
    assert len(np.unique(inputs[:24], axis=0)) == 24
    agent = Agent(scales)
    agent.load_state_dict(torch.load(checkpoint, weights_only=True)["agent"])
    np.testing.assert_array_equal(outputs, agent.act(inputs))
    exported = forward(policy, inputs)
    np.testing.assert_allclose(exported[:24], outputs[:24], rtol=0, atol=1e-5)
    np.testing.assert_allclose(exported[24:], outputs[24:], rtol=1e-5, atol=1e-5)
