from dataclasses import dataclass

import numpy as np
import torch

from feint_train.agent import Agent, DiscreteAgent


@dataclass
class UpdateConfig:
    epochs: int = 4
    minibatches: int = 8
    clip: float = 0.2
    entropy: float = 0.003
    value_coef: float = 0.5
    max_grad: float = 0.5


@dataclass
class UpdateStats:
    policy_loss: float
    value_loss: float
    entropy: float
    approx_kl: float
    clip_fraction: float


def minibatches(order: np.ndarray, parts: int) -> list[np.ndarray]:
    return [part for part in np.array_split(order, parts) if len(part)]


def ppo_update(
    agent: Agent | DiscreteAgent,
    optimizer: torch.optim.Optimizer,
    data: dict[str, np.ndarray],
    advantage: np.ndarray,
    returns: np.ndarray,
    config: UpdateConfig,
    generator: np.random.Generator,
) -> UpdateStats:
    device = agent.scales.device
    obs = torch.as_tensor(data["obs"], device=device)
    action = torch.as_tensor(data["action"], device=device)
    old_logp = torch.as_tensor(data["logp"], device=device)
    legal = data.get("legal")
    masks = None if legal is None else torch.as_tensor(legal, device=device)
    adv = torch.as_tensor(advantage, dtype=torch.float32, device=device)
    ret = torch.as_tensor(returns, dtype=torch.float32, device=device)
    count = len(obs)
    totals = np.zeros(5)
    batches = 0
    for _ in range(config.epochs):
        for part in minibatches(generator.permutation(count), config.minibatches):
            index = torch.as_tensor(part, device=device)
            dist = agent.policy(obs[index], None if masks is None else masks[index])
            logp = agent.log_prob(dist, action[index])
            ratio = (logp - old_logp[index]).exp()
            a = adv[index]
            a = (a - a.mean()) / (a.std(unbiased=False) + 1e-8)
            policy_loss = torch.max(
                -a * ratio, -a * ratio.clamp(1 - config.clip, 1 + config.clip)
            ).mean()
            value_loss = 0.5 * (agent.value(obs[index]) - ret[index]).pow(2).mean()
            entropy = agent.entropy(dist).mean()
            loss = policy_loss + config.value_coef * value_loss - config.entropy * entropy
            optimizer.zero_grad()
            loss.backward()
            torch.nn.utils.clip_grad_norm_(agent.parameters(), config.max_grad)
            optimizer.step()
            with torch.no_grad():
                log_ratio = logp - old_logp[index]
                kl = ((ratio - 1) - log_ratio).mean()
                clipped = ((ratio - 1).abs() > config.clip).float().mean()
            totals += [
                policy_loss.item(),
                value_loss.item(),
                entropy.item(),
                kl.item(),
                clipped.item(),
            ]
            batches += 1
    return UpdateStats(*(totals / max(1, batches)))
