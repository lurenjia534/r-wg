# Linux Full-Tunnel `RX=0` 排查与结论

## 症状

在 Linux 上使用 `r-wg` 连接某些 WireGuard 节点时，可能出现以下现象：

- 隧道显示已启动
- `Handshake` 持续更新
- `TX` / Upload 有增长
- `RX` / Download 长期为 `0`
- `ping 1.1.1.1` / `ping 2606:4700:4700::1111` 不通

这不是单纯的 UI 统计问题；在当前实现里，`rx_bytes` 只会在成功解密并写回 TUN 后增加。

## 已做过的排除

以下方向已被排除或明显降级：

- UI 统计显示错误
- DNS 配置问题
- 流量根本没有进入隧道
- Linux policy routing 根本没有生效
- 缺少 `wg-quick` `CONNMARK` 规则就是唯一主因
- `gotatun` Linux `sendmmsg` 路径本身

## 最终 A/B 结果

最终确定，这个问题在当前项目依赖图下，表现为 `gotatun 0.4.0` 与较新的
`zerocopy` 解析结果之间的组合回归，而不是单一组件独立失效。

已验证组合：

- `gotatun 0.2.0`：可用
- `gotatun 0.3.1 + zerocopy 0.8.42`：可用
- `gotatun 0.4.0 + zerocopy 0.8.27`：可用
- `gotatun 0.4.0 + zerocopy 0.8.33`：不可用
- `gotatun 0.4.0 + zerocopy 0.8.37`：不可用
- `gotatun 0.4.0 + zerocopy 0.8.40`：不可用

## 结论

当前工作结论是：

> `gotatun 0.4.0` 在本项目依赖图下，与 `zerocopy >= 0.8.33` 的组合会触发 Linux 全隧道数据面回归；而 `gotatun 0.3.1` 在 `zerocopy 0.8.42` 下仍可正常工作。

因此，`2026-03-17` 发布的 `r-wg 0.2.7` 选择的稳定方案是：

- 回退 `gotatun` 到 `0.3.1`
- 保持 `zerocopy 0.8.42`
- 保持 `zerocopy-derive 0.8.42`

问题边界更接近以下“组合变化”：

- `gotatun 0.4.0 + zerocopy 0.8.27`
- `gotatun 0.4.0 + zerocopy >= 0.8.33`

而不是单独某一个组件升级就必然失效。

## 当前 workaround

当前仓库采用的工作组合是：

- `gotatun 0.3.1`
- `zerocopy 0.8.42`
- `zerocopy-derive 0.8.42`

这里固定的是根项目 `Cargo.lock` 的解析结果，而不是修改 `gotatun` 上游仓库的 `Cargo.lock`。

## 诊断脚本

为复现与记录该问题，仓库中保留了两份 Linux 侧辅助脚本：

- `scripts/linux/collect_diag.sh`
  - 采集路由、规则、接口状态、`wg show`、`rp_filter`、`src_valid_mark`、`nft`/`iptables` 等诊断信息
- `scripts/linux/test_connmark_nft.sh`
  - 临时模拟 `wg-quick` 全隧道路径里的部分 `nft` / `connmark` 行为

## 后续建议

如果后续还要继续追根因，而不是只保留 workaround，优先级建议如下：

1. 在 `gotatun 0.4.0` 前提下，继续 bisect `zerocopy 0.8.28..0.8.32`
2. 整理最小复现并提交给上游
3. 在上游修复前，继续保持 `gotatun 0.3.1`
