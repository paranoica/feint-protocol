from dataclasses import dataclass, field

import numpy as np


def weighted_goals(reward: np.ndarray, concede_weight: float) -> np.ndarray:
    return np.where(reward < 0.0, reward * concede_weight, reward).astype(reward.dtype)


@dataclass
class Transition:
    slot: int
    obs: np.ndarray
    action: np.ndarray
    logp: float
    value: float
    reward: float = 0.0
    terminal: bool = False
    linked: bool = False
    next_value: float = 0.0
    closed: bool = False
    legal: np.ndarray | None = None


@dataclass
class Collector:
    slots: int
    gamma: float
    potential_scale: float
    shaping: float = 0.0
    turns: bool = False
    transitions: list[Transition] = field(default_factory=list)
    open: dict[int, Transition] = field(default_factory=dict)
    order: dict[int, list[int]] = field(default_factory=dict)

    def potential(self, obs: np.ndarray) -> float:
        return self.shaping * float(obs[0]) / self.potential_scale

    def act(
        self,
        ready: np.ndarray,
        obs: np.ndarray,
        actions: np.ndarray,
        logps: np.ndarray,
        values: np.ndarray,
        legal: np.ndarray | None = None,
    ) -> None:
        for slot in np.flatnonzero(ready):
            item = Transition(
                slot=int(slot),
                obs=obs[slot].copy(),
                action=np.array(actions[slot]).copy(),
                logp=float(logps[slot]),
                value=float(values[slot]),
                legal=None if legal is None else legal[slot].copy(),
            )
            self.open[int(slot)] = item
            self.order.setdefault(int(slot), []).append(len(self.transitions))
            self.transitions.append(item)

    def credit(
        self,
        out: dict[str, np.ndarray],
        values: np.ndarray,
        final_values: np.ndarray,
    ) -> None:
        reward = out["reward"].reshape(-1)
        ready = out["ready"].reshape(-1)
        obs = out["obs"].reshape(self.slots, -1)
        final_obs = out["final_obs"].reshape(self.slots, -1)
        terminated = np.repeat(out["terminated"], 2)
        truncated = np.repeat(out["truncated"], 2)
        for slot, item in list(self.open.items()):
            item.reward += float(reward[slot])
            if terminated[slot]:
                item.reward -= self.potential(item.obs)
                item.terminal = True
                item.next_value = 0.0
            elif truncated[slot]:
                item.reward += self.gamma * self.potential(final_obs[slot]) - self.potential(
                    item.obs
                )
                item.next_value = float(final_values[slot])
            elif ready[slot]:
                item.reward += self.gamma * self.potential(obs[slot]) - self.potential(item.obs)
                item.next_value = float(values[slot])
                item.linked = True
            elif self.turns:
                continue
            else:
                raise RuntimeError(f"slot {slot} decided and then neither decided nor ended")
            item.closed = True
            del self.open[slot]

    def advantages(self, lam: float) -> tuple[np.ndarray, np.ndarray]:
        count = len(self.transitions)
        advantage = np.zeros(count, dtype=np.float64)
        for indices in self.order.values():
            following = 0.0
            linked_to_next = False
            for index in reversed(indices):
                item = self.transitions[index]
                if not item.closed:
                    raise RuntimeError("an open transition has no outcome yet")
                bootstrap = 0.0 if item.terminal else item.next_value
                delta = item.reward + self.gamma * bootstrap - item.value
                carry = following if (item.linked and linked_to_next) else 0.0
                advantage[index] = delta + self.gamma * lam * carry
                following = advantage[index]
                linked_to_next = True
        values = np.array([t.value for t in self.transitions], dtype=np.float64)
        return advantage, advantage + values

    def arrays(self) -> dict[str, np.ndarray]:
        out = {
            "obs": np.stack([t.obs for t in self.transitions]).astype(np.float32),
            "action": np.stack([t.action for t in self.transitions]).astype(np.float32),
            "logp": np.array([t.logp for t in self.transitions], dtype=np.float32),
            "value": np.array([t.value for t in self.transitions], dtype=np.float32),
        }
        if self.transitions and self.transitions[0].legal is not None:
            out["legal"] = np.stack([t.legal for t in self.transitions]).astype(np.uint8)
        return out

    def clear(self) -> None:
        if self.open:
            raise RuntimeError("clearing with open transitions")
        self.transitions = []
        self.order = {}
