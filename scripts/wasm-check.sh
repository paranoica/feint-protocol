#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

cargo build -q --locked --release -p airhockey-check --lib --target wasm32-unknown-unknown
cargo build -q --locked -p feint-ai -p airhockey -p feint-ml --target wasm32-unknown-unknown
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

cargo build -q --locked --release -p feint-ml-check --lib --target wasm32-unknown-unknown
ml_debug="$(cargo run -q --locked -p feint-ml-check)"
ml_release="$(cargo run -q --locked --release -p feint-ml-check)"
ml_wasm="$(node crates/feint-ml-check/check.mjs \
  target/wasm32-unknown-unknown/release/feint_ml_check.wasm)"

echo "trained native debug:   $ml_debug"
echo "trained native release: $ml_release"
echo "trained wasm32:         $ml_wasm"
if [[ "$ml_debug" != "$ml_release" || "$ml_debug" != "$ml_wasm" ]]; then
  echo "the trained policy is not deterministic across builds" >&2
  exit 1
fi

scripts/web-build.sh --wasm-only
record="target/web-check.record"
trained_record="target/web-check-trained.record"
{
  read -r browser rate
  read -r trained_browser
  read -r rerun_browser rerun_level rerun_seed rerun_seat
} < <(node scripts/web-check.mjs "$root/web/src/wasm/feint_web.js" \
  web/src/wasm/feint_web_bg.wasm "$record" "$trained_record" web/public/fakes.json)
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

trained_native="$(cargo run -q --locked --release -p feint-arena --bin verify-record -- "$trained_record")"
echo "browser trained record: $trained_browser"
echo "arena verify (native):  $trained_native"
if [[ "$trained_browser" != "$trained_native" ]]; then
  echo "a browser match against the trained bot does not verify natively" >&2
  exit 1
fi

if [[ "$rerun_browser" != "none" ]]; then
  rerun_native="$(cargo run -q --locked --release -p feint-compare --bin rerun -- "$rerun_level" "$rerun_seed" "$rerun_seat")"
  echo "browser replay link:    $rerun_browser (level $rerun_level, seed $rerun_seed, seat $rerun_seat)"
  echo "native harness:         $rerun_native"
  if [[ "$rerun_browser" != "$rerun_native" ]]; then
    echo "a replay link does not rerun the same match in the browser" >&2
    exit 1
  fi
fi

cargo test -q --locked --release -p feint-web --test rerun -- --ignored
