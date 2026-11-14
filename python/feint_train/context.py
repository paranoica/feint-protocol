import argparse
import json
import math
import time
from pathlib import Path

import numpy as np
import torch
from safetensors import safe_open
from safetensors.numpy import save_file
from torch import nn

FORMAT = "feint-context-v1"
FEATURES = 20
KICKS = 10
OUTPUTS = 8
LN_EPS = 1e-5


def layout(game: str = "penalty") -> str:
    return f"context-v1:{game}:{FEATURES}:{KICKS}:{OUTPUTS}"


class Block(nn.Module):
    def __init__(self, width: int, heads: int) -> None:
        super().__init__()
        self.heads = heads
        self.ln1 = nn.LayerNorm(width, eps=LN_EPS)
        self.qkv = nn.Linear(width, 3 * width)
        self.out = nn.Linear(width, width)
        self.ln2 = nn.LayerNorm(width, eps=LN_EPS)
        self.ff1 = nn.Linear(width, 4 * width)
        self.ff2 = nn.Linear(4 * width, width)

    def attend(self, x: torch.Tensor) -> torch.Tensor:
        b, k, w = x.shape
        d = w // self.heads
        q, key, v = self.qkv(x).split(w, dim=-1)
        q = q.view(b, k, self.heads, d).transpose(1, 2)
        key = key.view(b, k, self.heads, d).transpose(1, 2)
        v = v.view(b, k, self.heads, d).transpose(1, 2)
        scores = q @ key.transpose(-1, -2) / math.sqrt(d)
        mask = torch.ones(k, k, dtype=torch.bool, device=x.device).triu(1)
        scores = scores.masked_fill(mask, float("-inf"))
        mixed = torch.softmax(scores, dim=-1) @ v
        return self.out(mixed.transpose(1, 2).reshape(b, k, w))

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        x = x + self.attend(self.ln1(x))
        return x + self.ff2(torch.relu(self.ff1(self.ln2(x))))


class ContextModel(nn.Module):
    def __init__(self, width: int = 48, layers: int = 2, heads: int = 4) -> None:
        super().__init__()
        self.width, self.layers, self.heads = width, layers, heads
        self.start = nn.Parameter(torch.zeros(FEATURES))
        self.embed = nn.Linear(FEATURES, width)
        self.pos = nn.Parameter(torch.randn(KICKS, width) * 0.02)
        self.blocks = nn.ModuleList(Block(width, heads) for _ in range(layers))
        self.ln_f = nn.LayerNorm(width, eps=LN_EPS)
        self.head = nn.Linear(width, OUTPUTS)

    def forward(self, tokens: torch.Tensor) -> torch.Tensor:
        x = self.embed(shifted(tokens, self.start)) + self.pos[: tokens.shape[1]]
        for block in self.blocks:
            x = block(x)
        return self.head(self.ln_f(x))


def shifted(tokens: torch.Tensor, start: torch.Tensor) -> torch.Tensor:
    first = start.expand(tokens.shape[0], 1, -1)
    return torch.cat([first, tokens[:, :-1]], dim=1)


def losses(
    logits: torch.Tensor, targets: torch.Tensor
) -> tuple[torch.Tensor, dict[str, torch.Tensor]]:
    parts: dict[str, torch.Tensor] = {}
    zero = logits.new_zeros(())
    for name, sl, t in [("shot", slice(0, 3), 0), ("tell", slice(3, 6), 1)]:
        y = targets[..., t]
        seen = y >= 0
        parts[name] = (
            nn.functional.cross_entropy(logits[..., sl][seen], y[seen]) if seen.any() else zero
        )
    for name, i, t in [("guess", 6, 2), ("follow", 7, 3)]:
        y = targets[..., t]
        seen = y >= 0
        parts[name] = (
            nn.functional.binary_cross_entropy_with_logits(logits[..., i][seen], y[seen].float())
            if seen.any()
            else zero
        )
    return sum(parts.values(), zero), parts


def count_baseline(shots: np.ndarray) -> np.ndarray:
    n, k = shots.shape
    out = np.zeros((n, k, 3))
    counts = np.ones((n, 3))
    for t in range(k):
        out[:, t] = counts / counts.sum(axis=1, keepdims=True)
        seen = shots[:, t] >= 0
        counts[seen, shots[seen, t]] += 1.0
    return out


def tensors_of(model: ContextModel) -> dict[str, np.ndarray]:
    return {
        name: p.detach().cpu().numpy().astype(np.float32) for name, p in model.state_dict().items()
    }


def export_context(model: ContextModel, path: Path) -> None:
    metadata = {
        "format": FORMAT,
        "layout": layout(),
        "width": str(model.width),
        "layers": str(model.layers),
        "heads": str(model.heads),
        "activation": "relu",
        "ln_eps": str(LN_EPS),
    }
    path.parent.mkdir(parents=True, exist_ok=True)
    save_file(
        {k: np.ascontiguousarray(v) for k, v in tensors_of(model).items()}, str(path), metadata
    )


def load_context(path: Path) -> ContextModel:
    with safe_open(str(path), framework="numpy") as f:
        meta = f.metadata()
        if meta.get("format") != FORMAT or meta.get("layout") != layout():
            raise ValueError(f"not a {FORMAT} {layout()} file: {meta}")
        model = ContextModel(int(meta["width"]), int(meta["layers"]), int(meta["heads"]))
        names = f.keys()
        state = {k: torch.from_numpy(f.get_tensor(k).copy()) for k in names}
    model.load_state_dict(state)
    return model


def load_data(folder: Path) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    tokens = np.load(folder / "tokens.npy")
    targets = np.load(folder / "targets.npy").astype(np.int64)
    habits = np.load(folder / "habits.npy")
    return tokens, targets, habits


def per_position(model: ContextModel, tokens: np.ndarray, targets: np.ndarray) -> list[float]:
    model.eval()
    with torch.no_grad():
        logits = model(torch.from_numpy(tokens))
        y = torch.from_numpy(targets)
        return [
            float(losses(logits[:, t : t + 1], y[:, t : t + 1])[1]["shot"]) for t in range(KICKS)
        ]


def shot_curves(targets: np.ndarray, habits: np.ndarray) -> dict[str, list[float]]:
    shots = targets[..., 0]
    counts = count_baseline(shots)
    oracle = habits[:, :3] / habits[:, :3].sum(axis=1, keepdims=True)
    prior = np.full(3, 1.0 / 3.0)
    curves: dict[str, list[float]] = {"prior": [], "counts": [], "oracle": []}
    for t in range(KICKS):
        seen = shots[:, t] >= 0
        y = shots[seen, t]
        curves["prior"].append(float(-np.log(prior[y]).mean()))
        curves["counts"].append(float(-np.log(counts[seen, t][np.arange(len(y)), y]).mean()))
        curves["oracle"].append(
            float(-np.log(np.maximum(oracle[seen][np.arange(len(y)), y], 1e-9)).mean())
        )
    return curves


def train(args: argparse.Namespace) -> None:
    torch.manual_seed(args.seed)
    torch.set_num_threads(args.threads)
    parts = [load_data(folder) for folder in args.data]
    all_tokens = np.concatenate([p[0] for p in parts])
    all_targets = np.concatenate([p[1] for p in parts])
    held = max(1, len(all_tokens) // 20)
    tokens, targets = all_tokens[held:], all_targets[held:]
    val_tokens, val_targets = all_tokens[:held], all_targets[:held]
    test_tokens, test_targets, test_habits = load_data(args.test)
    model = ContextModel(args.width, args.layers, args.heads)
    optimizer = torch.optim.AdamW(model.parameters(), lr=args.lr, weight_decay=0.01)
    x = torch.from_numpy(tokens)
    y = torch.from_numpy(targets)
    vx = torch.from_numpy(val_tokens)
    vy = torch.from_numpy(val_targets)
    best = float("inf")
    args.out.mkdir(parents=True, exist_ok=True)
    log = (args.out / "train.jsonl").open("w")
    started = time.time()
    for epoch in range(args.epochs):
        model.train()
        order = torch.randperm(len(x))
        for i in range(0, len(x), args.batch):
            idx = order[i : i + args.batch]
            total, _ = losses(model(x[idx]), y[idx])
            optimizer.zero_grad()
            total.backward()
            optimizer.step()
        model.eval()
        with torch.no_grad():
            total, parts = losses(model(vx), vy)
        row = {
            "epoch": epoch,
            "validation": float(total),
            **{k: float(v) for k, v in parts.items()},
        }
        row["seconds"] = round(time.time() - started, 1)
        log.write(json.dumps(row) + "\n")
        log.flush()
        print(json.dumps(row), flush=True)
        if float(total) < best:
            best = float(total)
            export_context(model, args.out / "context.safetensors")
    best_model = load_context(args.out / "context.safetensors")
    curves = shot_curves(test_targets, test_habits)
    curves["context"] = per_position(best_model, test_tokens, test_targets)
    print("shot log loss by kick index (test persons):")
    for name, values in curves.items():
        print(f"  {name:8s} " + " ".join(f"{v:.3f}" for v in values))
    (args.out / "curves.json").write_text(json.dumps(curves))


def parse(argv: list[str] | None = None) -> argparse.Namespace:
    p = argparse.ArgumentParser(prog="feint_train.context")
    p.add_argument("--data", type=Path, nargs="+", required=True)
    p.add_argument("--test", type=Path, required=True)
    p.add_argument("--out", type=Path, default=Path("runs/context"))
    p.add_argument("--epochs", type=int, default=30)
    p.add_argument("--batch", type=int, default=512)
    p.add_argument("--lr", type=float, default=1e-3)
    p.add_argument("--width", type=int, default=48)
    p.add_argument("--layers", type=int, default=2)
    p.add_argument("--heads", type=int, default=4)
    p.add_argument("--seed", type=int, default=1)
    p.add_argument("--threads", type=int, default=6)
    return p.parse_args(argv)


if __name__ == "__main__":
    train(parse())


def fixture(folder: Path) -> str:
    torch.manual_seed(7)
    model = ContextModel(width=16, layers=2, heads=2).eval()
    with torch.no_grad():
        for p in model.parameters():
            p.add_(torch.randn_like(p) * 0.3)
    export_context(model, folder / "tiny.safetensors")
    g = torch.Generator().manual_seed(11)
    lines = []
    for n in [0, 1, 4, 9]:
        seq = torch.rand(1, KICKS, FEATURES, generator=g)
        seq[:, n:] = 0.0
        with torch.no_grad():
            out = model(seq)[0, n]
        lines.append(f"case {n}")
        lines.extend("token " + " ".join(f"{v:.9g}" for v in row) for row in seq[0, :n].tolist())
        lines.append("outputs " + " ".join(f"{float(v):.9g}" for v in out))
    text = "\n".join(lines) + "\n"
    (folder / "tiny.txt").write_text(text)
    return text
