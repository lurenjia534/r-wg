#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

build_args=()
run_args=()
pass_to_run=false
for arg in "$@"; do
  if [[ "$arg" == "--" ]]; then
    pass_to_run=true
    continue
  fi
  if $pass_to_run; then
    run_args+=("$arg")
  else
    build_args+=("$arg")
  fi
done

"${script_dir}/build_with_cap.sh" "${build_args[@]}"

profile="debug"
for arg in "${build_args[@]}"; do
  if [[ "$arg" == "--release" ]]; then
    profile="release"
  fi
done

bin="target/${profile}/r-wg"
exec "${bin}" "${run_args[@]}"
