#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

scripts/web-build.sh
cargo build -q --locked --release -p feint-arena
data="$(mktemp -d)"
log="$data/arena.log"
target/release/feint-arena --dist web/dist --data "$data" --port 0 > "$log" 2>&1 &
arena=$!
trap 'kill "$arena" 2>/dev/null || true; rm -rf "$data"' EXIT
for _ in $(seq 50); do
  grep -q "feint-arena on" "$log" && break
  sleep 0.1
done
base="$(sed -n 's/^feint-arena on \(http:.*\)$/\1/p' "$log")"
if [[ -z "$base" ]]; then
  cat "$log" >&2
  exit 1
fi
patched="$data/unknown-checkpoint.rec"
node scripts/arena-check.mjs "$base" "$root/web/src/wasm/feint_web.js" web/src/wasm/feint_web_bg.wasm "$patched"
if refusal="$(target/release/verify-record "$patched" 2>&1)"; then
  echo "a record naming an unknown checkpoint verifies natively" >&2
  exit 1
fi
[[ "$refusal" == *"Checkpoint"* ]] || { echo "the patched record fails for another reason: $refusal" >&2; exit 1; }
page="$(curl -fsS "$base/")"
[[ "$page" == *"Feint"* ]] || { echo "the arena does not serve the front" >&2; exit 1; }
parquet="$(find "$data/matches" -name '*.parquet' | wc -l)"
if [[ "$parquet" -ne 3 ]]; then
  echo "expected 3 Parquet files, found $parquet" >&2
  exit 1
fi
echo "arena-check: 3 matches verified and stored (one against the trained bot), profile chain 1 -> 2, unknown checkpoint refused, front served"
