#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
(cd python && uv python install -q)
PYO3_PYTHON="$(cd python && uv python find)"
export PYO3_PYTHON
(cd crates/feint-gym && cargo fmt --check && cargo clippy --locked -q -- -D warnings)
cd python
uv sync --locked --reinstall-package feint-gym -q
uv run ruff check .
uv run black --check -q .
uv run pytest
