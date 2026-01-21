#!/usr/bin/env bash
set -euo pipefail

profile=""
explicit_profile="false"
custom_profile=""

for arg in "$@"; do
  case "$arg" in
    --release)
      explicit_profile="true"
      profile="release"
      ;;
    --debug)
      explicit_profile="true"
      profile="debug"
      ;;
    --profile)
      explicit_profile="true"
      ;;
  esac
done

if [[ "${explicit_profile}" == "true" ]]; then
  prev=""
  for arg in "$@"; do
    if [[ "${prev}" == "--profile" ]]; then
      custom_profile="${arg}"
      break
    fi
    prev="${arg}"
  done
  if [[ -n "${custom_profile}" ]]; then
    profile="${custom_profile}"
  fi
fi

if [[ -z "${profile}" ]]; then
  if [[ -t 0 && -t 1 ]]; then
    if [[ -t 1 ]]; then
      BOLD=$'\033[1m'
      BLUE=$'\033[34m'
      GREEN=$'\033[32m'
      RESET=$'\033[0m'
    else
      BOLD=""
      BLUE=""
      GREEN=""
      RESET=""
    fi

    echo "Select build profile:"
    echo "  ${BOLD}${BLUE}1)${RESET} ${BOLD}${BLUE}Debug${RESET}"
    echo "  ${BOLD}${GREEN}2)${RESET} ${BOLD}${GREEN}Release${RESET}"
    while true; do
      read -r -p "Enter choice [1-2]: " choice
      case "${choice}" in
        1)
          profile="debug"
          break
          ;;
        2)
          profile="release"
          break
          ;;
        *)
          echo "Invalid selection, choose 1 or 2."
          ;;
      esac
    done
  else
    echo "No TTY detected. Use --release or --profile <name>." >&2
    profile="debug"
  fi
fi

build_args=()
for arg in "$@"; do
  case "$arg" in
    --debug)
      ;;
    *)
      build_args+=("${arg}")
      ;;
  esac
done

if [[ "${profile}" == "release" && "${explicit_profile}" != "true" ]]; then
  build_args+=("--release")
fi

cargo build "${build_args[@]}"

bin="target/${profile}/r-wg"
if [[ ! -f "${bin}" ]]; then
  echo "binary not found: ${bin}" >&2
  exit 1
fi

sudo setcap cap_net_admin+ep "${bin}"
echo "setcap ok: ${bin}"
