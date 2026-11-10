import numpy as np
import torch

from feint_train.agent import Agent
from feint_train.update import UpdateConfig, ppo_update


def target(obs: np.ndarray) -> np.ndarray:
    return np.stack([0.5 * obs[:, 0], -0.5 * obs[:, 1]], axis=-1)


def error(agent: Agent, obs: np.ndarray) -> float:
    return float(np.abs(agent.act(obs) - target(obs)).mean())


def test_ppo_moves_the_mean_action_towards_what_pays() -> None:
    torch.manual_seed(0)
    rng = np.random.default_rng(0)
    agent = Agent(np.ones(4, dtype=np.float32))
    optimizer = torch.optim.Adam(agent.parameters(), lr=3e-3)
    probe = rng.uniform(-1, 1, (512, 4)).astype(np.float32)
    before = error(agent, probe)
    for _ in range(40):
        obs = rng.uniform(-1, 1, (1024, 4)).astype(np.float32)
        action, logp, value = agent.sample(obs)
        reward = -np.square(action - target(obs)).sum(-1)
        data = {"obs": obs, "action": action, "logp": logp, "value": value}
        ppo_update(agent, optimizer, data, reward - value, reward, UpdateConfig(), rng)
    after = error(agent, probe)
    assert after < 0.4 * before, (before, after)


def test_the_value_head_learns_the_return() -> None:
    torch.manual_seed(1)
    rng = np.random.default_rng(1)
    agent = Agent(np.ones(4, dtype=np.float32))
    optimizer = torch.optim.Adam(agent.parameters(), lr=3e-3)
    obs = rng.uniform(-1, 1, (1024, 4)).astype(np.float32)
    returns = 2.0 * obs[:, 2]
    for _ in range(30):
        action, logp, value = agent.sample(obs)
        data = {"obs": obs, "action": action, "logp": logp, "value": value}
        ppo_update(agent, optimizer, data, np.zeros(len(obs)), returns, UpdateConfig(), rng)
    assert np.abs(agent.values(obs) - returns).mean() < 0.15
