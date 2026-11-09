#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

want_cbindgen="0.29.4"
have_cbindgen="$(cbindgen --version | awk '{print $2}')"
if [[ "$have_cbindgen" != "$want_cbindgen" ]]; then
  echo "cbindgen $want_cbindgen required, found $have_cbindgen" >&2
  exit 1
fi
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
cbindgen --quiet --config cbindgen.toml --crate feint-ffi --output "$work/feint.h"
if ! diff -u include/feint.h "$work/feint.h"; then
  echo "include/feint.h is stale: regenerate it with cbindgen" >&2
  exit 1
fi
cargo build -q --locked --release -p feint-ffi
libs="$(cargo rustc -q --locked --release -p feint-ffi --lib --crate-type staticlib -- --print native-static-libs 2>&1 \
  | sed -n 's/.*native-static-libs: //p' | tail -1)"
gcc -std=c11 -Wall -Wextra -Werror -pedantic -Iinclude examples/c/match.c \
  target/release/libfeint_ffi.a $libs -o "$work/match"
from_c="$("$work/match")"
native="$(cargo run -q --locked --release -p feint-ffi --bin native)"
echo "C:      $from_c"
echo "native: $native"
if [[ "$from_c" != "$native" ]]; then
  echo "the C example does not play the native match" >&2
  exit 1
fi
