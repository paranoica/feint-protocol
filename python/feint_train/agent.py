import numpy as np
import torch
from torch import nn

HIDDEN = 256


def mlp(inputs: int, outputs: int, hidden: int = HIDDEN) -> nn.Sequential:
    return nn.Sequential(
        nn.Linear(inputs, hidden),
        nn.ReLU(),
        nn.Linear(hidden, hidden),
        nn.ReLU(),
        nn.Linear(hidden, outputs),
    )


class Agent(nn.Module):
    def __init__(self, scales: np.ndarray, outputs: int = 2, std: float = 0.3) -> None:
        super().__init__()
        features = len(scales)
        self.register_buffer("scales", torch.as_tensor(scales, dtype=torch.float32))
        self.actor = mlp(features, outputs)
        self.critic = mlp(features, 1)
        self.log_std = nn.Parameter(torch.full((outputs,), float(np.log(std))))
        nn.init.orthogonal_(self.actor[-1].weight, gain=0.01)
        nn.init.zeros_(self.actor[-1].bias)
        nn.init.orthogonal_(self.critic[-1].weight, gain=1.0)

    def inverse_scales(self) -> torch.Tensor:
        return 1.0 / self.scales

    def normalise(self, obs: torch.Tensor) -> torch.Tensor:
        return obs * self.inverse_scales()

    def value(self, obs: torch.Tensor) -> torch.Tensor:
        return self.critic(self.normalise(obs)).squeeze(-1)

    def mean(self, obs: torch.Tensor) -> torch.Tensor:
        return self.actor(self.normalise(obs))

    def distribution(self, obs: torch.Tensor) -> torch.distributions.Normal:
        return torch.distributions.Normal(self.mean(obs), self.log_std.exp())

    def policy(
        self, obs: torch.Tensor, legal: torch.Tensor | None = None
    ) -> torch.distributions.Normal:
        return self.distribution(obs)

    def log_prob(self, dist: torch.distributions.Normal, action: torch.Tensor) -> torch.Tensor:
        return dist.log_prob(action).sum(-1)

    def entropy(self, dist: torch.distributions.Normal) -> torch.Tensor:
        return dist.entropy().sum(-1)

    @torch.no_grad()
    def sample(self, obs: np.ndarray) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
        x = torch.as_tensor(obs, device=self.scales.device)
        dist = self.distribution(x)
        action = dist.sample()
        logp = dist.log_prob(action).sum(-1)
        return action.cpu().numpy(), logp.cpu().numpy(), self.value(x).cpu().numpy()

    @torch.no_grad()
    def values(self, obs: np.ndarray) -> np.ndarray:
        return self.value(torch.as_tensor(obs, device=self.scales.device)).cpu().numpy()

    @torch.no_grad()
    def act(self, obs: np.ndarray) -> np.ndarray:
        return self.mean(torch.as_tensor(obs, device=self.scales.device)).cpu().numpy()


class DiscreteAgent(nn.Module):
    def __init__(self, scales: np.ndarray, actions: int, hidden: int = HIDDEN) -> None:
        super().__init__()
        features = len(scales)
        self.register_buffer("scales", torch.as_tensor(scales, dtype=torch.float32))
        self.actor = mlp(features, actions, hidden)
        self.critic = mlp(features, 1, hidden)
        nn.init.orthogonal_(self.actor[-1].weight, gain=0.01)
        nn.init.zeros_(self.actor[-1].bias)
        nn.init.orthogonal_(self.critic[-1].weight, gain=1.0)

    def inverse_scales(self) -> torch.Tensor:
        return 1.0 / self.scales

    def normalise(self, obs: torch.Tensor) -> torch.Tensor:
        return obs * self.inverse_scales()

    def value(self, obs: torch.Tensor) -> torch.Tensor:
        return self.critic(self.normalise(obs)).squeeze(-1)

    def logits(self, obs: torch.Tensor) -> torch.Tensor:
        return self.actor(self.normalise(obs))

    def policy(
        self, obs: torch.Tensor, legal: torch.Tensor | None = None
    ) -> torch.distributions.Categorical:
        logits = self.logits(obs)
        if legal is not None:
            logits = logits.masked_fill(legal == 0, torch.finfo(logits.dtype).min)
        return torch.distributions.Categorical(logits=logits)

    def log_prob(self, dist: torch.distributions.Categorical, action: torch.Tensor) -> torch.Tensor:
        return dist.log_prob(action.long())

    def entropy(self, dist: torch.distributions.Categorical) -> torch.Tensor:
        return dist.entropy()

    @torch.no_grad()
    def sample(
        self, obs: np.ndarray, legal: np.ndarray
    ) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
        device = self.scales.device
        x = torch.as_tensor(obs, device=device)
        dist = self.policy(x, torch.as_tensor(legal, device=device))
        action = dist.sample()
        logp = dist.log_prob(action)
        return action.cpu().numpy(), logp.cpu().numpy(), self.value(x).cpu().numpy()

    @torch.no_grad()
    def values(self, obs: np.ndarray) -> np.ndarray:
        return self.value(torch.as_tensor(obs, device=self.scales.device)).cpu().numpy()

    @torch.no_grad()
    def probabilities(self, obs: np.ndarray, legal: np.ndarray) -> np.ndarray:
        device = self.scales.device
        dist = self.policy(
            torch.as_tensor(obs, device=device), torch.as_tensor(legal, device=device)
        )
        return dist.probs.cpu().numpy()
