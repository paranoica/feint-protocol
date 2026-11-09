#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

cargo build -q --locked --release -p airhockey-check --lib --target wasm32-unknown-unknown
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

scripts/web-build.sh --wasm-only
record="target/web-check.record"
read -r browser rate < <(node scripts/web-check.mjs "$root/web/src/wasm/feint_web.js" \
  web/src/wasm/feint_web_bg.wasm "$record")
native="$(cargo run -q --locked --release -p feint-web --bin replay -- "$record")"
echo "browser record: $browser at $rate ticks/s"
echo "native replay:  $native"
if [[ "$browser" != "$native" ]]; then
  echo "a browser record does not replay to the same match natively" >&2
  exit 1
fi
if (( rate < 960 )); then
  echo "the wasm match runs at $rate ticks/s, below 4x real time (960)" >&2
  exit 1
fi
