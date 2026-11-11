import os
import warnings
from pathlib import Path

import pytest
import torch

from feint_train.train import TrainConfig, Trainer, hide_gpu_for
from feint_train.update import UpdateStats

DECISION_HZ = 60


def small(seed: int = 1) -> TrainConfig:
    return TrainConfig(envs=4, threads=1, torch_threads=1, seed=seed)


def test_the_default_discount_looks_about_seventeen_seconds_ahead() -> None:
    horizon_s = 1.0 / (1.0 - TrainConfig().gamma) / DECISION_HZ
    assert 16.0 < horizon_s < 17.5


def test_starting_from_a_checkpoint_takes_its_weights_and_nothing_else(tmp_path: Path) -> None:
    source = Trainer(small(seed=1), tmp_path / "a")
    with torch.no_grad():
        source.agent.log_std.fill_(-1.25)
    source.updates = 7
    source.start -= 3600.0
    source.optimizer.state[source.agent.log_std] = {"step": torch.tensor(3.0)}
    source.checkpoint("policy")

    fresh = Trainer(small(seed=2), tmp_path / "b")
    started = Trainer(small(seed=2), tmp_path / "c", init=tmp_path / "a" / "checkpoint.pt")

    assert not torch.equal(fresh.agent.actor[0].weight, source.agent.actor[0].weight)
    taken, given = started.agent.state_dict(), source.agent.state_dict()
    assert taken.keys() == given.keys()
    for name in given:
        assert torch.equal(taken[name], given[name])
    assert started.updates == 0
    assert started.elapsed() < 60.0
    assert len(started.optimizer.state) == 0


def test_resuming_and_starting_from_a_checkpoint_do_not_mix(tmp_path: Path) -> None:
    with pytest.raises(ValueError):
        Trainer(small(), tmp_path, resume=tmp_path / "x.pt", init=tmp_path / "y.pt")


def test_cpu_training_hides_the_gpu_and_gpu_training_leaves_it(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setenv("CUDA_VISIBLE_DEVICES", "0")
    hide_gpu_for("cuda")
    assert os.environ["CUDA_VISIBLE_DEVICES"] == "0"
    hide_gpu_for("cpu")
    assert os.environ["CUDA_VISIBLE_DEVICES"] == ""


def test_logging_an_update_raises_no_warnings(tmp_path: Path) -> None:
    trainer = Trainer(small(), tmp_path)
    stats = UpdateStats(
        policy_loss=0.0, value_loss=0.0, entropy=0.0, approx_kl=0.0, clip_fraction=0.0
    )
    with warnings.catch_warnings():
        warnings.simplefilter("error")
        trainer.log(stats, rollout=1.0, update=1.0, transitions=10)


def test_training_and_evaluation_seeds_must_differ(tmp_path: Path) -> None:
    config = small(seed=77)
    assert config.eval_seed == 77
    with pytest.raises(ValueError):
        Trainer(config, tmp_path)


def test_a_failed_checkpoint_save_keeps_the_previous_checkpoint(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    trainer = Trainer(small(), tmp_path)
    trainer.updates = 3
    trainer.checkpoint("first")

    def broken(obj: object, f: object) -> None:
        Path(str(f)).write_bytes(b"half a checkpoint")
        raise OSError("disk full")

    monkeypatch.setattr(torch, "save", broken)
    with pytest.raises(OSError):
        trainer.checkpoint("second")
    assert torch.load(tmp_path / "checkpoint.pt")["updates"] == 3


def test_one_iteration_collects_updates_and_clears(tmp_path: Path) -> None:
    trainer = Trainer(small(), tmp_path)
    trainer.config.steps = 8
    before = [p.detach().clone() for p in trainer.agent.parameters()]
    trainer.iteration(0.0)
    assert trainer.updates == 1
    assert trainer.collector.transitions == [] and not trainer.collector.open
    after = list(trainer.agent.parameters())
    assert any(not torch.equal(a, b) for a, b in zip(after, before, strict=True))
