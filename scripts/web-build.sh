#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

want="$(grep -A1 '^name = "wasm-bindgen"$' Cargo.lock | sed -n 's/^version = "\(.*\)"$/\1/p')"
have="$(wasm-bindgen --version | awk '{print $2}')"
if [[ "$want" != "$have" ]]; then
  echo "wasm-bindgen CLI is $have, the crate is $want" >&2
  exit 1
fi
cargo build -q --locked --release -p feint-web --lib --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir web/src/wasm \
  target/wasm32-unknown-unknown/release/feint_web.wasm
if [[ "${1:-}" != "--wasm-only" ]]; then
  (cd web && npm run build --silent)
fi
