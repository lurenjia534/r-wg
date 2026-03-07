#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  sudo scripts/linux/test_connmark_nft.sh apply <iface> <fwmark>
  sudo scripts/linux/test_connmark_nft.sh cleanup <iface>
  sudo scripts/linux/test_connmark_nft.sh status <iface>

Examples:
  sudo scripts/linux/test_connmark_nft.sh apply us-lax-wg-407 0x5257
  sudo scripts/linux/test_connmark_nft.sh status us-lax-wg-407
  sudo scripts/linux/test_connmark_nft.sh cleanup us-lax-wg-407

Notes:
  - This is an experiment script to emulate the wg-quick full-tunnel nftables
    connmark rules on Linux.
  - It requires root because nftables state is modified via `nft`.
  - The script does not start or stop the tunnel. Run it while the tunnel is up.
EOF
}

require_root() {
  if [[ "${EUID}" -ne 0 ]]; then
    echo "must run as root" >&2
    exit 1
  fi
}

require_nft() {
  if ! command -v nft >/dev/null 2>&1; then
    echo "nft not found" >&2
    exit 1
  fi
}

sanitize_name() {
  local value="$1"
  value="${value//[^a-zA-Z0-9_]/_}"
  printf '%s\n' "${value}"
}

table_name_for() {
  local iface="$1"
  printf 'rwg_connmark_%s\n' "$(sanitize_name "${iface}")"
}

collect_ipv4_addrs() {
  local iface="$1"
  ip -o -4 addr show dev "${iface}" scope global 2>/dev/null | awk '{print $4}' | cut -d/ -f1
}

collect_ipv6_addrs() {
  local iface="$1"
  ip -o -6 addr show dev "${iface}" scope global 2>/dev/null | awk '{print $4}' | cut -d/ -f1
}

delete_table_if_exists() {
  local family="$1"
  local table="$2"
  if nft list table "${family}" "${table}" >/dev/null 2>&1; then
    nft delete table "${family}" "${table}"
  fi
}

cleanup_tables() {
  local iface="$1"
  local table
  table="$(table_name_for "${iface}")"
  delete_table_if_exists ip "${table}"
  delete_table_if_exists ip6 "${table}"
}

apply_family_table() {
  local family="$1"
  local iface="$2"
  local fwmark="$3"
  local table="$4"
  shift 4
  local addrs=("$@")

  if [[ "${#addrs[@]}" -eq 0 ]]; then
    return 0
  fi

  local addr_expr="ip"
  if [[ "${family}" == "ip6" ]]; then
    addr_expr="ip6"
  fi

  {
    echo "add table ${family} ${table}"
    echo "add chain ${family} ${table} preraw { type filter hook prerouting priority -300; }"
    echo "add chain ${family} ${table} premangle { type filter hook prerouting priority -150; }"
    echo "add chain ${family} ${table} postmangle { type filter hook postrouting priority -150; }"
    for addr in "${addrs[@]}"; do
      echo "add rule ${family} ${table} preraw iifname != \"${iface}\" ${addr_expr} daddr ${addr} fib saddr type != local drop"
    done
    echo "add rule ${family} ${table} postmangle meta l4proto udp mark ${fwmark} ct mark set mark"
    echo "add rule ${family} ${table} premangle meta l4proto udp meta mark set ct mark"
  } | nft -f -
}

cmd_apply() {
  local iface="$1"
  local fwmark_raw="$2"
  local fwmark_dec
  local table
  local -a v4_addrs=()
  local -a v6_addrs=()

  fwmark_dec="$((fwmark_raw))"
  table="$(table_name_for "${iface}")"

  mapfile -t v4_addrs < <(collect_ipv4_addrs "${iface}")
  mapfile -t v6_addrs < <(collect_ipv6_addrs "${iface}")

  cleanup_tables "${iface}"
  apply_family_table ip "${iface}" "${fwmark_dec}" "${table}" "${v4_addrs[@]}"
  apply_family_table ip6 "${iface}" "${fwmark_dec}" "${table}" "${v6_addrs[@]}"
  sysctl -q net.ipv4.conf.all.src_valid_mark=1

  echo "applied nft connmark experiment for ${iface} (fwmark=${fwmark_raw}/${fwmark_dec})"
  if [[ "${#v4_addrs[@]}" -gt 0 ]]; then
    echo "ipv4 tunnel addrs: ${v4_addrs[*]}"
  fi
  if [[ "${#v6_addrs[@]}" -gt 0 ]]; then
    echo "ipv6 tunnel addrs: ${v6_addrs[*]}"
  fi
}

cmd_cleanup() {
  local iface="$1"
  cleanup_tables "${iface}"
  echo "cleaned nft connmark experiment for ${iface}"
}

cmd_status() {
  local iface="$1"
  local table
  table="$(table_name_for "${iface}")"
  nft list table ip "${table}" 2>/dev/null || true
  nft list table ip6 "${table}" 2>/dev/null || true
}

main() {
  if [[ $# -lt 2 ]]; then
    usage >&2
    exit 1
  fi

  require_root
  require_nft

  local command="$1"
  shift

  case "${command}" in
    apply)
      if [[ $# -ne 2 ]]; then
        usage >&2
        exit 1
      fi
      cmd_apply "$1" "$2"
      ;;
    cleanup)
      if [[ $# -ne 1 ]]; then
        usage >&2
        exit 1
      fi
      cmd_cleanup "$1"
      ;;
    status)
      if [[ $# -ne 1 ]]; then
        usage >&2
        exit 1
      fi
      cmd_status "$1"
      ;;
    *)
      usage >&2
      exit 1
      ;;
  esac
}

main "$@"
