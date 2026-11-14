#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

if ! command -v godot > /dev/null; then
  echo "godot-check: skipped (godot not found)"
  exit 0
fi
version="$(godot --version | cut -d. -f1,2)"
if [[ "$version" != "4.7" ]]; then
  echo "Godot 4.7 required, found $(godot --version)" >&2
  exit 1
fi
cargo build -q --locked --release -p feint-ffi
libs="$(cargo rustc -q --locked --release -p feint-ffi --lib --crate-type staticlib -- --print native-static-libs 2>&1 \
  | sed -n 's/.*native-static-libs: //p' | tail -1)"
project=examples/godot
mkdir -p "$project/bin"
gcc -std=c11 -Wall -Wextra -Werror -Wno-cast-function-type -O2 -fPIC -fvisibility=hidden -shared \
  -Iinclude -I"$project/extension" "$project/extension/feint_godot.c" "$project/extension/feint_match.c" \
  target/release/libfeint_ffi.a $libs -Wl,--exclude-libs,ALL \
  -o "$project/bin/libfeint_godot.so"
exported="$(nm -D --defined-only "$project/bin/libfeint_godot.so" | awk '{print $3}' | grep -v '^_' || true)"
if [[ "$exported" != "feint_godot_init" ]]; then
  echo "the extension exports more than its entry point: $exported" >&2
  exit 1
fi
if ! editor="$(timeout 120 godot --headless -e --path "$project" --quit-after 300 2>&1)"; then
  echo "the Godot project does not import" >&2
  exit 1
fi
code=0
out="$(timeout 60 godot --headless --rendering-driver dummy --audio-driver Dummy --path "$project" -- --check 2>&1)" || code=$?
line="$(sed -n 's/^FEINT //p' <<< "$out" | tail -1)"
native="$(cargo run -q --locked --release -p feint-ffi --bin native)"
echo "Godot:  $line"
echo "native: $native"
if grep -qi "leaked" <<< "$editor$out"; then
  echo "Godot leaked objects" >&2
  grep -i leaked <<< "$editor$out" >&2
  exit 1
fi
if grep -q "SCRIPT ERROR" <<< "$out"; then
  echo "the check script failed" >&2
  grep -A3 "SCRIPT ERROR" <<< "$out" >&2
  exit 1
fi
if [[ "$code" != 0 || -z "$line" || "$line" != "$native" ]]; then
  echo "the Godot-hosted match is not the native match (exit $code)" >&2
  echo "$out" | tail -20 >&2
  exit 1
fi
