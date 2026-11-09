#!/usr/bin/env bash
# The scripted match must give the same digest natively (debug and release) and as wasm32.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

cargo build -q --locked --release -p airhockey-check --lib --target wasm32-unknown-unknown
# The core and the hockey game (with its playbook) must also build for the browser.
cargo build -q --locked -p feint-ai -p airhockey --target wasm32-unknown-unknown
debug="$(cargo run -q --locked -p airhockey-check)"
release="$(cargo run -q --locked --release -p airhockey-check)"
wasm="$(node crates/games/airhockey-check/check.mjs \
  target/wasm32-unknown-unknown/release/airhockey_check.wasm)"

echo "native debug:   $debug"
echo "native release: $release"
echo "wasm32:         $wasm"
if [[ "$debug" != "$release" || "$debug" != "$wasm" ]]; then
  echo "physics is not deterministic across builds" >&2
  exit 1
fi
