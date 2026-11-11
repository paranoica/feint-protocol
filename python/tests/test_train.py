import os
import pickle
import signal
import subprocess
import sys
import time
import warnings
from pathlib import Path

import numpy as np
import pytest
import torch

from feint_train.league import League
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


class Planted:
    pass


@pytest.mark.parametrize("flag", ["resume", "init"])
def test_a_checkpoint_carrying_arbitrary_objects_is_refused_even_when_torch_is_told_to_allow_them(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, flag: str
) -> None:
    source = Trainer(small(), tmp_path / "a")
    planted = tmp_path / "planted.pt"
    torch.save(
        {
            "agent": source.agent.state_dict(),
            "optimizer": source.optimizer.state_dict(),
            "spent": 0.0,
            "updates": 0,
            "planted": Planted(),
        },
        planted,
    )
    monkeypatch.setenv("TORCH_FORCE_NO_WEIGHTS_ONLY_LOAD", "1")
    with pytest.raises(pickle.UnpicklingError):
        Trainer(small(), tmp_path / "b", **{flag: planted})


def league_config(seed: int = 1) -> TrainConfig:
    return TrainConfig(
        envs=10,
        threads=1,
        torch_threads=1,
        seed=seed,
        steps=8,
        league=True,
        eval_opponents=["level:1"],
    )


def test_resume_carries_the_clock_updates_league_and_rng(tmp_path: Path) -> None:
    first = Trainer(league_config(), tmp_path)
    first_seed = first.batch_seed
    first.iteration(0.0)
    first.snapshot()
    first.league.record(first.league.ids()[0], 0.75)
    first.next_generation_now()
    first.iteration(0.0)
    first.start -= 600.0
    first.save()
    saved = torch.load(tmp_path / "checkpoint.pt", weights_only=True)
    torch_state = torch.get_rng_state()
    numpy_state = first.rng.bit_generator.state

    again = Trainer(league_config(), tmp_path, resume=tmp_path / "checkpoint.pt")
    assert again.updates == first.updates == 2
    assert again.elapsed() >= 600.0
    assert again.league.ids() == first.league.ids()
    assert again.league.rate(first.league.ids()[0]) == first.league.rate(first.league.ids()[0])
    assert again.generation == saved["generation"] + 1
    assert again.batch_seed not in (first_seed, first.batch_seed)
    assert torch.equal(torch.get_rng_state(), torch_state)
    replay = np.random.default_rng()
    replay.bit_generator.state = numpy_state
    expected = League.from_state(saved["league"]).table(10, replay)
    assert again.table == expected.seats
    assert again.rng.bit_generator.state == replay.bit_generator.state
    assert again.next_snapshot == saved["next_snapshot"]
    assert again.next_eval == saved["next_eval"]


def test_a_generation_rebuilds_the_batch_with_a_new_seed_and_frozen_seats(tmp_path: Path) -> None:
    trainer = Trainer(league_config(), tmp_path)
    seen = {trainer.batch_seed}
    trainer.iteration(0.0)
    trainer.snapshot()
    trainer.next_generation_now()
    assert trainer.batch_seed not in seen
    assert any(s.startswith("frozen:") for pair in trainer.table for s in pair)
    trainer.iteration(0.0)
    assert trainer.updates == 2


def test_a_killed_run_resumes_from_its_last_checkpoint(tmp_path: Path) -> None:
    command = [
        sys.executable,
        "-m",
        "feint_train.train",
        "--logdir",
        str(tmp_path),
        "--league",
        "--minutes",
        "1.2",
        "--envs",
        "10",
        "--threads",
        "1",
        "--torch-threads",
        "1",
        "--steps",
        "8",
        "--generation-minutes",
        "0.15",
        "--snapshot-minutes",
        "0.15",
        "--eval-every",
        "100",
        "--eval-matches",
        "2",
        "--resume",
        "auto",
    ]
    env = {**os.environ, "CUDA_VISIBLE_DEVICES": ""}
    run = subprocess.Popen(command, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    checkpoint = tmp_path / "checkpoint.pt"
    deadline = time.monotonic() + 120
    while time.monotonic() < deadline:
        if checkpoint.exists() and torch.load(checkpoint, weights_only=True)["generation"] >= 2:
            break
        time.sleep(0.5)
    run.send_signal(signal.SIGKILL)
    run.wait()
    killed = torch.load(checkpoint, weights_only=True)
    assert killed["generation"] >= 2 and killed["league"]["archive"]
    finished = subprocess.run(command, env=env, capture_output=True, timeout=300)
    assert finished.returncode == 0, finished.stderr.decode()[-2000:]
    final = torch.load(checkpoint, weights_only=True)
    assert final["updates"] > killed["updates"]
    assert final["spent"] >= 1.2 * 60
    assert final["generation"] > killed["generation"]
    assert (tmp_path / "policy-final.safetensors").exists()


def test_stats_value_capped_matches_by_the_lead() -> None:
    from feint_train.train import Stats

    stats = Stats()
    stats.record("frozen", 3, 1, finished=False)
    stats.record("frozen", 1, 3, finished=False)
    stats.record("frozen", 7, 2, finished=True)
    assert list(stats.results["frozen"]) == [0.75, 0.25, 1.0]


def test_results_against_an_evicted_snapshot_go_to_the_frozen_bucket(tmp_path: Path) -> None:
    trainer = Trainer(league_config(), tmp_path)
    trainer.snapshot()
    kept = trainer.league.ids()[0]
    trainer.account(kept, 3, 1, finished=False)
    trainer.league.archive.clear()
    trainer.account(kept, 1, 3, finished=False)
    trainer.account("level:5", 0, 7, finished=True)
    assert set(trainer.stats.results) == {"frozen", "level:5"}
    assert list(trainer.stats.results["frozen"]) == [0.75, 0.25]
