import numpy as np
import torch
from torch import nn

HIDDEN = 256


def mlp(inputs: int, outputs: int) -> nn.Sequential:
    return nn.Sequential(
        nn.Linear(inputs, HIDDEN),
        nn.ReLU(),
        nn.Linear(HIDDEN, HIDDEN),
        nn.ReLU(),
        nn.Linear(HIDDEN, outputs),
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
