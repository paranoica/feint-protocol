#!/usr/bin/env bash
set -euo pipefail

FLATC_VERSION="25.12.19"
root="$(cd "$(dirname "$0")/.." && pwd)"
target="$root/crates/feint-protocol/src/generated"

FLATC_SHA256="7d16cbedc9721dc6985a810438038531cd8e4e6e11630af707d3a4c0c40f91f4"
pinned="${HOME:-}/.local/share/flatc/$FLATC_VERSION/flatc"
flatc="${FLATC:-}"
if [[ -n "$flatc" ]]; then
  flatc="$(command -v "$flatc" || true)"
else
  if [[ -x "$pinned" ]]; then
    flatc="$pinned"
  else
    flatc="$(command -v flatc || true)"
  fi
fi
if [[ -z "$flatc" ]] || [[ "$(sha256sum "$flatc" | awk '{print $1}')" != "$FLATC_SHA256" ]]; then
  echo "flatc must be the official $FLATC_VERSION release binary (sha256 $FLATC_SHA256)," >&2
  echo "found '${flatc:-none}'; distro builds patch the generator. Put the release binary at" >&2
  echo "$pinned or point FLATC at it." >&2
  exit 1
fi

out="$(mktemp -d)"
trap 'rm -rf "$out"' EXIT
"$flatc" --rust --gen-object-api -o "$out" "$root/schema/feint.fbs"

if [[ "${1:-}" == "--check" ]]; then
  if ! diff -u "$target/feint_generated.rs" "$out/feint_generated.rs"; then
    echo "generated protocol code is stale: run scripts/gen-protocol.sh" >&2
    exit 1
  fi
else
  cp "$out/feint_generated.rs" "$target/feint_generated.rs"
fi
