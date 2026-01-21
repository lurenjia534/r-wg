#!/usr/bin/env bash
set -euo pipefail

profile="debug"
for arg in "$@"; do
  if [[ "$arg" == "--release" ]]; then
    profile="release"
  fi
done

cargo build "$@"

bin="target/${profile}/r-wg"
if [[ ! -f "${bin}" ]]; then
  echo "binary not found: ${bin}" >&2
  exit 1
fi

sudo setcap cap_net_admin+ep "${bin}"
echo "setcap ok: ${bin}"
