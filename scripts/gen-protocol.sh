#!/usr/bin/env bash
# Regenerate the Rust protocol types from schema/feint.fbs.
# --check: fail when the committed file differs from a fresh generation.
set -euo pipefail

FLATC_VERSION="25.12.19"
root="$(cd "$(dirname "$0")/.." && pwd)"
target="$root/crates/feint-protocol/src/generated"

actual="$(flatc --version | awk '{print $3}')"
if [[ "$actual" != "$FLATC_VERSION" ]]; then
  echo "flatc $FLATC_VERSION required (matches the flatbuffers crate), found $actual" >&2
  exit 1
fi

out="$(mktemp -d)"
trap 'rm -rf "$out"' EXIT
flatc --rust --gen-object-api -o "$out" "$root/schema/feint.fbs"

if [[ "${1:-}" == "--check" ]]; then
  if ! diff -u "$target/feint_generated.rs" "$out/feint_generated.rs"; then
    echo "generated protocol code is stale: run scripts/gen-protocol.sh" >&2
    exit 1
  fi
else
  cp "$out/feint_generated.rs" "$target/feint_generated.rs"
fi
