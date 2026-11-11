import argparse
import json
from pathlib import Path

import numpy as np
from safetensors.numpy import load_file, save_file

from feint_train.evaluate import Result, evaluate
from feint_train.export import forward, metadata

SELECTION_SEED = 31_337


def best_of(scored: dict[str, Result]) -> str:
    return max(
        scored,
        key=lambda name: (scored[name].wins, scored[name].goals_for - scored[name].goals_against),
    )


def candidates(logdir: Path, top: int, opponent: str = "level:5") -> list[str]:
    scored: dict[str, Result] = {}
    for line in (logdir / "evals.jsonl").read_text().splitlines():
        row = json.loads(line)
        if row["opponent"] == opponent:
            scored[row["policy"]] = Result(
                opponent=opponent,
                wins=row["wins"],
                losses=row["losses"],
                unfinished=row["unfinished"],
                goals_for=row["goals_for"],
                goals_against=row["goals_against"],
            )
    ranked = sorted(
        scored,
        key=lambda name: (scored[name].wins, scored[name].goals_for - scored[name].goals_against),
        reverse=True,
    )
    return [name for name in ranked if (logdir / f"{name}.safetensors").exists()][:top]


def select(
    logdir: Path,
    top: int = 3,
    matches: int = 200,
    opponent: str = "level:5",
    threads: int = 6,
) -> str:
    scored: dict[str, Result] = {}
    for name in candidates(logdir, top):
        path = logdir / f"{name}.safetensors"
        scored[name] = evaluate(
            lambda obs, path=path: np.clip(forward(path, obs), -1.0, 1.0),
            opponent,
            matches,
            SELECTION_SEED,
            threads,
        )
        r = scored[name]
        print(
            f"{name}: {r.wins}/{r.matches} won, {r.losses} lost, {r.unfinished} unfinished, "
            f"goals {r.goals_for}:{r.goals_against}",
            flush=True,
        )
    chosen = best_of(scored)
    source = logdir / f"{chosen}.safetensors"
    r = scored[chosen]
    meta = {
        **metadata(source),
        "selected_from": chosen,
        "selected_against": opponent,
        "selected_matches": str(r.matches),
        "selected_wins": str(r.wins),
        "selected_goals": f"{r.goals_for}:{r.goals_against}",
    }
    save_file(load_file(str(source)), str(logdir / "best.safetensors"), meta)
    print(f"best: {chosen} -> {logdir / 'best.safetensors'}", flush=True)
    return chosen


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--logdir", type=Path, required=True)
    parser.add_argument("--top", type=int, default=3)
    parser.add_argument("--matches", type=int, default=200)
    parser.add_argument("--threads", type=int, default=6)
    args = parser.parse_args()
    select(args.logdir, args.top, args.matches, threads=args.threads)


if __name__ == "__main__":
    main()
