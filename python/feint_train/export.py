from pathlib import Path

import numpy as np
from safetensors import safe_open
from safetensors.numpy import load_file, save_file

from feint_train.agent import HIDDEN, Agent

FORMAT = "feint-mlp-v1"
LAYOUT = "airhockey-v3:anticipated-observation16+reaction_s+physique"


def export(agent: Agent, path: Path, level: str = "") -> None:
    actor = agent.actor
    tensors = {
        "scales": agent.scales.detach().cpu().numpy().astype(np.float32),
        "layer0.weight": actor[0].weight.detach().cpu().numpy().astype(np.float32),
        "layer0.bias": actor[0].bias.detach().cpu().numpy().astype(np.float32),
        "layer1.weight": actor[2].weight.detach().cpu().numpy().astype(np.float32),
        "layer1.bias": actor[2].bias.detach().cpu().numpy().astype(np.float32),
        "layer2.weight": actor[4].weight.detach().cpu().numpy().astype(np.float32),
        "layer2.bias": actor[4].bias.detach().cpu().numpy().astype(np.float32),
    }
    metadata = {
        "format": FORMAT,
        "layout": LAYOUT,
        "features": str(len(tensors["scales"])),
        "outputs": str(len(tensors["layer2.bias"])),
        "hidden": f"{HIDDEN},{HIDDEN}",
        "activation": "relu",
        "level": level,
    }
    path.parent.mkdir(parents=True, exist_ok=True)
    save_file({k: np.ascontiguousarray(v) for k, v in tensors.items()}, str(path), metadata)


def metadata(path: Path) -> dict[str, str]:
    with safe_open(str(path), framework="numpy") as f:
        return dict(f.metadata() or {})


def forward(path: Path, obs: np.ndarray) -> np.ndarray:
    t = load_file(str(path))
    x = obs.astype(np.float32) / t["scales"]
    x = np.maximum(x @ t["layer0.weight"].T + t["layer0.bias"], 0.0)
    x = np.maximum(x @ t["layer1.weight"].T + t["layer1.bias"], 0.0)
    return x @ t["layer2.weight"].T + t["layer2.bias"]
