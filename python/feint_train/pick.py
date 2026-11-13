import argparse
import json
from pathlib import Path


def ranked(logdir: Path, opponents: tuple[str, ...]) -> list[tuple[int, float, Path]]:
    shares: dict[int, dict[str, float]] = {}
    with (logdir / "evals.jsonl").open() as log:
        for line in log:
            row = json.loads(line)
            if row.get("opponent") in opponents:
                share = 0.5 + float(row["mean_reward"]) / 2.0
                shares.setdefault(int(row["update"]), {})[row["opponent"]] = share
    out = []
    for update, by in shares.items():
        path = logdir / f"policy-{update:05d}.safetensors"
        if set(by) == set(opponents) and path.exists():
            out.append((update, min(by.values()), path))
    return sorted(out, key=lambda c: (-c[1], -c[0]))


def main() -> None:
    parser = argparse.ArgumentParser(description="Rank exported turn policies by their worst share")
    parser.add_argument("logdir", type=Path)
    parser.add_argument("--opponents", default="honest,liar:300,caller:300,counter")
    parser.add_argument("--top", type=int, default=3)
    args = parser.parse_args()
    opponents = tuple(o for o in args.opponents.split(",") if o)
    for update, share, path in ranked(args.logdir, opponents)[: args.top]:
        print(f"{update} {share:.3f} {path}")


if __name__ == "__main__":
    main()
