# gotatun 本地补丁指南（内存 vs 性能）

目的
- 当频繁 start/stop 导致 RSS 台阶明显、且内存更敏感时，可考虑用本地补丁降低预分配。
- 代价：吞吐峰值与突发缓冲能力可能下降，高负载下更容易出现排队/分配抖动。

为什么默认值很大
- gotatun 预分配大块 buffer，降低运行期分配次数、减少锁竞争与抖动。
- 大缓冲能吸收突发流量，降低 backpressure。
- GRO/recvmmsg 使用大批量缓冲以减少 syscalls，提高高吞吐场景效率。

如何启用本地补丁
1) 复制 gotatun 源码到仓库内
   - 从 `~/.cargo/registry/src/.../gotatun-0.2.0` 复制到 `vendor/gotatun`
2) 在 `Cargo.toml` 添加 patch
   - 在 `[patch.crates-io]` 下添加：
     - `gotatun = { path = "vendor/gotatun" }`
3) 调整预分配常量（推荐只改这些，最小改动）
   - `vendor/gotatun/src/device/mod.rs`
     - `MAX_PACKET_BUFS`（影响 PacketBufPool 与多路缓冲容量）
   - `vendor/gotatun/src/udp/socket/linux.rs`
     - `MAX_PACKET_COUNT`（每次 recvmmsg/sendmmsg 批量数）
     - `MAX_SEGMENTS`（GRO 聚合段数，影响单次 GRO 缓冲大小）
4) （可选）显式 stop/await 缓冲任务
   - 目标：stop 时尽快退出后台缓冲任务，减少“旧任务未退场 + 新任务已创建”的叠加期。
   - 方向：为 BufferedIp/Udp 的后台任务提供显式 stop/await 句柄，并在 Connection::stop 中统一 await。

具体代码实现（示例）
1) `Cargo.toml` 增加本地 patch
```toml
[patch.crates-io]
gotatun = { path = "vendor/gotatun" }
```

2) 缩小 PacketBufPool 预分配
`vendor/gotatun/src/device/mod.rs`
```rust
// before: const MAX_PACKET_BUFS: usize = 4000;
const MAX_PACKET_BUFS: usize = 2048;
```

3) 缩小 Linux GRO 预分配
`vendor/gotatun/src/udp/socket/linux.rs`
```rust
const MAX_PACKET_COUNT: usize = 100;
// before: const MAX_SEGMENTS: usize = 64;
const MAX_SEGMENTS: usize = 32;
const MAX_GRO_SIZE: usize = MAX_SEGMENTS * 4096;
```

4) （可选）显式 stop/await 缓冲任务（核心片段）
`vendor/gotatun/src/task.rs`
```rust
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone, Default)]
pub struct TaskStopper {
    task: Arc<Mutex<Option<Task>>>,
}

impl TaskStopper {
    pub fn new(task: Task) -> Self {
        Self { task: Arc::new(Mutex::new(Some(task))) }
    }

    pub async fn stop(&self) {
        if let Some(task) = self.task.lock().await.take() {
            task.stop().await;
        }
    }
}
```

`vendor/gotatun/src/udp/buffer.rs` / `vendor/gotatun/src/tun/buffer.rs`
```rust
pub struct BufferedUdpSend {
    stop: TaskStopper,
    send_tx_v4: mpsc::Sender<(Packet, SocketAddr)>,
    send_tx_v6: mpsc::Sender<(Packet, SocketAddr)>,
}

impl BufferedUdpSend {
    pub fn stop_handle(&self) -> TaskStopper {
        self.stop.clone()
    }
}
```

`vendor/gotatun/src/device/mod.rs`
```rust
struct BufferStops { ip_rx: TaskStopper, ip_tx: TaskStopper, udp_tx_v4: TaskStopper,
    udp_tx_v6: TaskStopper, udp_rx_v4: TaskStopper, udp_rx_v6: TaskStopper }

impl BufferStops {
    async fn stop(self) {
        join!(self.ip_rx.stop(), self.ip_tx.stop(), self.udp_tx_v4.stop(), self.udp_tx_v6.stop(),
            self.udp_rx_v4.stop(), self.udp_rx_v6.stop());
    }
}

async fn stop(self) {
    // 原有 incoming/outgoing/timers stop...
    buffer_stops.stop().await;
}
```

回滚补丁
1) 删除 `Cargo.toml` 中的 `gotatun` patch 行
2) 删除 `vendor/gotatun` 目录
3) 重新构建（Cargo 将回退使用 crates.io 版本）

风险提示
- 缓冲缩小可能降低吞吐或增加 CPU/分配压力。
- GRO 参数缩小可能削弱高吞吐下的优势。
- 建议在真实流量下做对比测试再决定最终值。
