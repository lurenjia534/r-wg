# UI Refactor Progress

这份文档说明本轮 UI 重构实际做了什么、为什么这样做，以及当前还剩哪些收口项。

## 目标

本轮工作的主目标不是“重写 UI”，而是把现有结构往下面这个方向推进：

- 保留 `WgApp` 作为 GPUI 根 entity / root coordinator
- 让页面与局部状态逐步收进 `ui/features/*`
- 让 `ui/view` 退回共享壳层
- 让 `ui/state` 退回全局状态与兼容出口
- 补一套最小可用的 GPUI typed action / key dispatch 骨架

## 本轮完成的改动

### 1. `configs` 已经基本形成 feature 闭环

`configs` 不再主要依赖旧的 `ui/view/configs*` 和 `ui/actions/config/*`。

现在 `configs` 的宿主主要在：

- `src/ui/features/configs/controller.rs`
- `src/ui/features/configs/import_export.rs`
- `src/ui/features/configs/storage.rs`
- `src/ui/features/configs/dialogs.rs`
- `src/ui/features/configs/app.rs`
- `src/ui/features/configs/state.rs`
- `src/ui/features/configs/view/*`

已经收进去的内容包括：

- 选择 / 加载 / 保存 / 删除 / 导入导出主流程
- configs 局部状态
- configs 页面渲染
- configs feature 专属的 `WgApp` helper

结果是：

- `src/ui/view/configs*` 已退场
- `src/ui/actions/config/*` 已实质退场
- `ui/state` 不再是 configs 局部状态的真实宿主

### 2. `route_map` 页面实现已迁入 feature

`route_map` 不再由旧 `src/ui/view/route_map/*` 持有真实页面实现。

当前主宿主在：

- `src/ui/features/route_map/view.rs`
- `src/ui/features/route_map/events.rs`
- `src/ui/features/route_map/inventory.rs`
- `src/ui/features/route_map/graph.rs`
- `src/ui/features/route_map/inspector.rs`
- `src/ui/features/route_map/data.rs`
- `src/ui/features/route_map/presenter.rs`
- `src/ui/features/route_map/explain.rs`

结果是：

- `src/ui/view/route_map/*` 已退场
- `route_map` 不再只是 feature 门面，而是 feature 宿主

### 3. `overview` 和 `tools` 已从“入口壳”推进到 feature 宿主

`overview` 现在已经在 `src/ui/features/overview/*` 下闭环。

`tools` 现在已经在 `src/ui/features/tools/*` 下闭环，包含：

- 页面渲染
- active config
- CIDR 行为
- reachability 行为
- audit 行为

旧的 `ui/actions/tools/*` 已删除。

### 4. `tools` 的 feature-local state 已迁回 feature

`tools` 这组局部状态之前仍挂在全局 `ui/state/tools/*`。

现在真实宿主已经迁到：

- `src/ui/features/tools/state/mod.rs`
- `src/ui/features/tools/state/active_config.rs`
- `src/ui/features/tools/state/cidr.rs`
- `src/ui/features/tools/state/reachability.rs`

`src/ui/state.rs` 目前只做兼容 re-export：

- 旧调用面不需要一次性重写
- feature 内部代码已经直接依赖自己的 `super::state`

### 5. 补上了最小可用的 GPUI typed action 骨架

当前统一的 action 宿主在：

- `src/ui/actions/app.rs`

这一层现在承载的 action 包括：

- `OpenOverview`
- `OpenConfigs`
- `OpenProxies`
- `OpenDns`
- `OpenLogs`
- `OpenRouteMap`
- `OpenTools`
- `OpenAdvanced`
- `OpenAbout`
- `ImportConfig`
- `SaveConfig`
- `ToggleTunnel`

已经接通的链路：

- `install_keybindings(cx)` 在应用启动时注册
- 根视图在 `src/ui/view/mod.rs` 统一挂 `on_action(...)`
- `Configs` 页补上了 `key_context("Configs")`
- 顶栏按钮、左侧导航、configs/proxies 的相关按钮都开始 dispatch typed action

这意味着当前已经不是“只有按钮回调，没有命令层”，而是有了一条实际工作的：

`actions! -> bind_keys -> key_context -> on_action -> WgApp handler`

## 当前结构判断

现在的 UI 已经比之前更接近下面这个形态：

- `ui/app.rs` 和 `WgApp` 继续做 root/shell 协调
- `ui/features/*` 成为页面和局部状态的真实宿主
- `ui/view/*` 主要保留共享壳层和少量通用页面
- `ui/state.rs` 开始退回兼容出口

也就是说，项目已经从“横切层为主”推进到“feature slice 为主”的过渡成熟态。

## 还没完成的部分

虽然方向已经基本正确，但还没有完全收口。

### 1. `WgApp` 仍然偏重

现在 `WgApp` 更像 root coordinator 了，但仍然承载了不少状态聚合和 mutation 入口。

后续仍可以继续把 feature-local 的编排和局部状态往各自 feature 下沉。

### 2. `ui/state/stores.rs` 仍然很重

这个文件仍然是明显热点，里面混合了多组状态：

- configs
- runtime
- stats
- ui prefs
- ui session
- persistence

后续适合继续拆小。

### 3. `ui/actions/persistence.rs` 仍然是旧横切层热点

虽然很多 feature 行为已经迁走了，但 persistence 还没被完全 feature 化或模块化。

### 4. action 体系还是“第一版骨架”

当前 action 体系已经可用，但还只是第一版。

还没有做的包括：

- 更细的 feature-local typed actions
- 菜单系统的系统化接入
- 更完整的 page/context 级 action 约束

## 验证状态

当前代码已经做过编译验证：

- `cargo check` 通过

当前仅剩一个已有告警：

- `src/ui/features/tools/state/reachability.rs`
- `ReachabilityAuditPhase::Completed` 目前未被构造

这不是本轮引入的问题，只是还没清理的旧告警。

## 一句话总结

这轮工作的核心成果不是“把 UI 全部改完”，而是把项目稳定推进到了：

**`AppRoot + feature-owned pages/state + shared shell + minimal GPUI action skeleton`**

后续最值得继续推进的两条线是：

1. 继续缩 `WgApp` 和 `ui/state/stores.rs`
2. 继续把 action 体系从“骨架”补成“真正的一等命令层”
