#!/usr/bin/env bash
set -euo pipefail

timestamp="$(date +%Y%m%d_%H%M%S)"
out="${1:-rwg_diag_${timestamp}.log}"

# 统一封装：记录命令与输出，失败也不中断脚本。
run() {
  local cmd="$1"
  {
    echo ""
    echo "### $cmd"
    eval "$cmd" || true
  } >>"$out" 2>&1
}

# 探测当前活跃的隧道接口：
# 1. 优先使用 `wg show interfaces`（若 gotatun 暴露了 UAPI）；
# 2. 否则回退到 POINTOPOINT 接口启发式。
detect_tunnel_iface() {
  local iface=""
  if command -v wg >/dev/null 2>&1; then
    iface="$(wg show interfaces 2>/dev/null | awk '{print $1; exit}')"
  fi
  if [[ -z "${iface}" ]]; then
    iface="$(ip -brief link 2>/dev/null | awk '$3 ~ /POINTOPOINT/ && $3 ~ /UP/ {print $1; exit}')"
  fi
  if [[ -z "${iface}" ]]; then
    iface="$(ip -brief link 2>/dev/null | awk '$3 ~ /POINTOPOINT/ {print $1; exit}')"
  fi
  printf '%s\n' "${iface}"
}

# 入口提示：建议在隧道已连接状态下执行。
echo "r-wg diag started: $(date)" >"$out"
echo "note: run this while the tunnel is CONNECTED" >>"$out"

# 基础环境信息。
run "uname -a"
run "id"
run "ip -brief link"
run "ip rule"
run "ip -6 rule"
run "ip route show"
run "ip -6 route show"
run "ip -d rule"
run "ip -6 -d rule"
run "ip route show table 200"
run "ip -6 route show table 200"
run "ip -d route show table 200"
run "ip -6 -d route show table 200"
run "ip -4 route get 1.1.1.1"
run "ip -6 route get 2a0d:5600:8:38::f001"
run "ip -6 route get 2a0d:5600:8:38::f001 mark 0x5257"
run "ip -4 route get 1.1.1.1 table 200"
run "ip -6 route get 2a0d:5600:8:38::f001 table 200"
run "ip -V"
run "ping -V"

# 当前系统默认路由接口及地址。
default_v4_iface="$(ip route show default 2>/dev/null | awk '/default/ {print $5; exit}')"
default_v6_iface="$(ip -6 route show default 2>/dev/null | awk '/default/ {print $5; exit}')"
tun_iface="$(detect_tunnel_iface)"

echo "detected_default_v4_iface=${default_v4_iface:-<none>}" >>"$out"
echo "detected_default_v6_iface=${default_v6_iface:-<none>}" >>"$out"
echo "detected_tunnel_iface=${tun_iface:-<none>}" >>"$out"

if [[ -n "${default_v4_iface}" ]]; then
  run "ip -4 addr show dev ${default_v4_iface}"
fi
if [[ -n "${default_v6_iface}" ]]; then
  run "ip -6 addr show dev ${default_v6_iface}"
fi
if [[ -n "${tun_iface}" ]]; then
  run "ip -4 route get 1.1.1.1 oif ${tun_iface}"
  run "ip -6 route get 2a0d:5600:8:38::f001 oif ${tun_iface}"
  run "ip -4 addr show dev ${tun_iface}"
  run "ip -6 addr show dev ${tun_iface}"
  run "ip -s link show dev ${tun_iface}"
fi

# ping/连通性与二进制信息。
run "command -v ping"
if command -v ping >/dev/null 2>&1; then
  run "ls -l \$(command -v ping)"
  run "ping -4 -c 3 -W 1 1.1.1.1"
  run "ping -6 -c 3 -W 1 2606:4700:4700::1111"
fi

# UDP 端口占用情况（握手/隧道诊断）。
run "command -v ss"
if command -v ss >/dev/null 2>&1; then
  run "ss -uapn"
  if command -v rg >/dev/null 2>&1; then
    run "ss -uapn | rg '51820|r-wg|gotatun|wg-backend'"
  else
    run "ss -uapn | grep -E '51820|r-wg|gotatun|wg-backend'"
  fi
fi

# WireGuard / 防火墙 / 内核路由判定辅助。
run "command -v wg"
if command -v wg >/dev/null 2>&1; then
  run "wg show"
  if [[ -n "${tun_iface}" ]]; then
    run "wg show ${tun_iface}"
    run "wg showconf ${tun_iface}"
  fi
fi

run "sysctl net.ipv4.conf.all.src_valid_mark"
run "sysctl net.ipv4.ping_group_range"
run "sysctl net.ipv4.conf.all.rp_filter"
run "sysctl net.ipv4.conf.default.rp_filter"
if [[ -n "${default_v4_iface}" ]]; then
  run "sysctl net.ipv4.conf.${default_v4_iface}.rp_filter"
fi
if [[ -n "${tun_iface}" ]]; then
  run "sysctl net.ipv4.conf.${tun_iface}.rp_filter"
fi

run "command -v nft"
if command -v nft >/dev/null 2>&1; then
  run "nft list ruleset"
fi

run "command -v iptables-save"
if command -v iptables-save >/dev/null 2>&1; then
  run "iptables-save"
fi

run "command -v ip6tables-save"
if command -v ip6tables-save >/dev/null 2>&1; then
  run "ip6tables-save"
fi

# DNS 后端检测。

run "command -v resolvectl"
if command -v resolvectl >/dev/null 2>&1; then
  run "resolvectl status"
fi
run "command -v resolvconf"
run "ls -l /etc/resolv.conf"
run "systemctl status systemd-resolved --no-pager"

echo ""
echo "diag saved to: ${out}"
