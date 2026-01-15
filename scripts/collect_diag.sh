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
run "ip -4 route get 1.1.1.1 oif test5"
run "ip -6 route get 2a0d:5600:8:38::f001 oif test5"
run "ip -4 addr show dev test5"
run "ip -6 addr show dev test5"
run "ip -V"
run "ping -V"

# 当前系统默认路由接口及地址。
default_v4_iface="$(ip route show default 2>/dev/null | awk '/default/ {print $5; exit}')"
default_v6_iface="$(ip -6 route show default 2>/dev/null | awk '/default/ {print $5; exit}')"
if [[ -n "${default_v4_iface}" ]]; then
  run "ip -4 addr show dev ${default_v4_iface}"
fi
if [[ -n "${default_v6_iface}" ]]; then
  run "ip -6 addr show dev ${default_v6_iface}"
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
  if command -v rg >/dev/null 2>&1; then
    run "ss -uapn | rg 51820"
  else
    run "ss -uapn | grep 51820"
  fi
fi

# DNS 后端检测。
run "sysctl net.ipv4.ping_group_range"

run "command -v resolvectl"
if command -v resolvectl >/dev/null 2>&1; then
  run "resolvectl status"
fi
run "command -v resolvconf"
run "ls -l /etc/resolv.conf"
run "systemctl status systemd-resolved --no-pager"

echo ""
echo "diag saved to: ${out}"
