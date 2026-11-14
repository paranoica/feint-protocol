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

cargo build -q --locked --release -p cards-check --lib --target wasm32-unknown-unknown
cards_debug="$(cargo run -q --locked -p cards-check)"
cards_release="$(cargo run -q --locked --release -p cards-check)"
cards_wasm="$(node crates/games/cards-check/check.mjs \
  target/wasm32-unknown-unknown/release/cards_check.wasm)"

echo "cards native debug:   $cards_debug"
echo "cards native release: $cards_release"
echo "cards wasm32:         $cards_wasm"
if [[ "$cards_debug" != "$cards_release" || "$cards_debug" != "$cards_wasm" ]]; then
  echo "a card game is not deterministic across builds" >&2
  exit 1
fi

search_release="$(cargo run -q --locked --release -p cards-check -- search 2>/dev/null)"
read -r search_wasm search_ms < <(node crates/games/cards-check/search.mjs \
  target/wasm32-unknown-unknown/release/cards_check.wasm)
echo "card search native release: $search_release"
echo "card search wasm32:         $search_wasm (slowest level $search_ms ms per move)"
if [[ "$search_release" != "$search_wasm" ]]; then
  echo "the card search is not deterministic across builds" >&2
  exit 1
fi
if (( search_ms > 200 )); then
  echo "a card search move takes $search_ms ms in wasm, over 200" >&2
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
card_record="target/web-check-cards.record"
penalty_record="target/web-check-penalty.record"
pingpong_record="target/web-check-pingpong.record"
boxing_record="target/web-check-boxing.record"
{
  read -r browser rate
  read -r trained_browser
  read -r rerun_browser rerun_level rerun_seed rerun_seat
  read -r card_browser
  read -r penalty_browser
  read -r pingpong_browser
  read -r boxing_browser
} < <(node scripts/web-check.mjs "$root/web/src/wasm/feint_web.js" \
  web/src/wasm/feint_web_bg.wasm "$record" "$trained_record" web/public/fakes.json "$card_record" \
  "$penalty_record" "$pingpong_record" "$boxing_record")
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

card_native="$(cargo run -q --locked --release -p feint-arena --bin verify-record -- "$card_record")"
echo "browser card record:    $card_browser"
echo "arena verify (native):  $card_native"
if [[ -z "$card_browser" || "$card_browser" != "$card_native" ]]; then
  echo "a browser card match does not verify natively" >&2
  exit 1
fi

penalty_native="$(cargo run -q --locked --release -p feint-arena --bin verify-record -- "$penalty_record")"
echo "browser level-4 shootout (context advisor): $penalty_browser"
echo "arena verify (native):                      $penalty_native"
if [[ -z "$penalty_browser" || "$penalty_browser" != "$penalty_native" ]]; then
  echo "a browser shootout with the context advisor does not verify natively" >&2
  exit 1
fi

pingpong_native="$(cargo run -q --locked --release -p feint-arena --bin verify-record -- "$pingpong_record")"
echo "browser table tennis:   $pingpong_browser"
echo "arena verify (native):  $pingpong_native"
if [[ -z "$pingpong_browser" || "$pingpong_browser" != "$pingpong_native" ]]; then
  echo "a browser table tennis match does not verify natively" >&2
  exit 1
fi

boxing_native="$(cargo run -q --locked --release -p feint-arena --bin verify-record -- "$boxing_record")"
echo "browser boxing:         $boxing_browser"
echo "arena verify (native):  $boxing_native"
if [[ -z "$boxing_browser" || "$boxing_browser" != "$boxing_native" ]]; then
  echo "a browser boxing match does not verify natively" >&2
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
