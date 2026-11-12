import argparse
import shutil
from pathlib import Path

import numpy as np
from safetensors.numpy import load_file, save_file

from feint_train.export import metadata


def with_inverse_scales(source: Path, target: Path) -> None:
    meta = metadata(source)
    target.parent.mkdir(parents=True, exist_ok=True)
    if "inv_scales" in meta:
        shutil.copyfile(source, target)
        return
    tensors = load_file(str(source))
    if "scales" not in tensors:
        raise ValueError(f"{source} carries neither inv_scales nor a scales tensor")
    inverse = np.float32(1.0) / tensors.pop("scales").astype(np.float32)
    meta["inv_scales"] = ",".join(np.format_float_positional(x, unique=True) for x in inverse)
    save_file(tensors, str(target), meta)


def gather(runs: Path, names: list[str], out: Path) -> list[Path]:
    written = []
    for run in names:
        for policy in sorted((runs / run).glob("policy-*.safetensors")):
            target = out / f"{run}__{policy.name}"
            with_inverse_scales(policy, target)
            written.append(target)
    return written


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--runs", type=Path, default=Path("runs"))
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("names", nargs="+")
    args = parser.parse_args()
    for path in gather(args.runs, args.names, args.out):
        print(path, flush=True)


if __name__ == "__main__":
    main()
