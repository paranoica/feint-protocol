from pathlib import Path

import numpy as np
import pytest
from safetensors.numpy import load_file, save_file

from feint_train.export import FORMAT, LAYOUT, loaded, metadata
from feint_train.levels import gather, with_inverse_scales

CHECKPOINT = Path(__file__).parents[2] / "crates/feint-ml/checkpoints/airhockey-g999.safetensors"


def old_style(path: Path, scales: np.ndarray) -> dict[str, np.ndarray]:
    rng = np.random.default_rng(5)
    tensors = {
        "layer0.weight": rng.normal(size=(4, 18)).astype(np.float32),
        "layer0.bias": rng.normal(size=4).astype(np.float32),
        "layer1.weight": rng.normal(size=(4, 4)).astype(np.float32),
        "layer1.bias": rng.normal(size=4).astype(np.float32),
        "layer2.weight": rng.normal(size=(2, 4)).astype(np.float32),
        "layer2.bias": rng.normal(size=2).astype(np.float32),
        "scales": scales,
    }
    meta = {"format": FORMAT, "layout": LAYOUT, "features": "18", "outputs": "2"}
    meta |= {"hidden": "4,4", "activation": "relu", "level": ""}
    save_file(tensors, str(path), meta)
    return tensors


def test_a_file_with_inverse_scales_is_copied_byte_for_byte(tmp_path: Path) -> None:
    with_inverse_scales(CHECKPOINT, tmp_path / "copy.safetensors")
    assert (tmp_path / "copy.safetensors").read_bytes() == CHECKPOINT.read_bytes()


def test_an_old_file_gets_inverse_scales_from_its_scales_tensor(tmp_path: Path) -> None:
    scales = np.array([2.0, 4.0, 0.5, 8.0, 1.0, 10.0] * 3, dtype=np.float32)
    t = old_style(tmp_path / "old.safetensors", scales)
    with_inverse_scales(tmp_path / "old.safetensors", tmp_path / "new.safetensors")
    meta = metadata(tmp_path / "new.safetensors")
    assert meta["inv_scales"].split(",")[:6] == ["0.5", "0.25", "2.", "0.125", "1.", "0.1"]
    assert meta["format"] == FORMAT and meta["layout"] == LAYOUT and meta["hidden"] == "4,4"
    assert "scales" not in load_file(str(tmp_path / "new.safetensors"))
    obs = np.random.default_rng(9).normal(size=(16, 18)).astype(np.float32) * 3
    x = np.maximum((obs / scales) @ t["layer0.weight"].T + t["layer0.bias"], 0.0)
    x = np.maximum(x @ t["layer1.weight"].T + t["layer1.bias"], 0.0)
    want = x @ t["layer2.weight"].T + t["layer2.bias"]
    got = loaded(tmp_path / "new.safetensors")(obs)
    np.testing.assert_allclose(got, want, rtol=1e-5, atol=1e-5)


def test_a_file_without_any_scales_is_refused(tmp_path: Path) -> None:
    old_style(tmp_path / "old.safetensors", np.ones(18, dtype=np.float32))
    tensors = load_file(str(tmp_path / "old.safetensors"))
    del tensors["scales"]
    save_file(tensors, str(tmp_path / "bare.safetensors"), metadata(tmp_path / "old.safetensors"))
    with pytest.raises(ValueError):
        with_inverse_scales(tmp_path / "bare.safetensors", tmp_path / "out.safetensors")
    assert not (tmp_path / "out.safetensors").exists()


def test_gather_names_each_candidate_by_run_and_policy(tmp_path: Path) -> None:
    runs = tmp_path / "runs"
    for run, names in {"a": ["policy-00010", "policy-final"], "b": ["policy-00020"]}.items():
        (runs / run).mkdir(parents=True)
        for name in names:
            (runs / run / f"{name}.safetensors").write_bytes(CHECKPOINT.read_bytes())
        (runs / run / "best.safetensors").write_bytes(CHECKPOINT.read_bytes())
    out = gather(runs, ["a", "b"], tmp_path / "out")
    assert [p.name for p in out] == [
        "a__policy-00010.safetensors",
        "a__policy-final.safetensors",
        "b__policy-00020.safetensors",
    ]
    assert all(p.read_bytes() == CHECKPOINT.read_bytes() for p in out)
