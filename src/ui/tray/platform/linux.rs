//! Linux 托盘实现（StatusNotifierItem + DBusMenu）。
//!
//! 兼容性说明：
//! - KDE：原生支持 SNI（Status Notifier Item）；
//! - GNOME：需启用 AppIndicator/StatusNotifier 类扩展后可显示托盘图标。
//!
//! 本模块负责 D-Bus 协议适配与平台行为，把点击动作转换成 `TrayCommand` 回传上层。

use std::collections::HashMap;
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, Instant};
use zbus::blocking::Connection;
use zbus::interface;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

use crate::ui::tray::types::TrayCommand;

/// SNI 服务名前缀，最终会拼上 PID，避免同名冲突。
const ITEM_SERVICE_PREFIX: &str = "org.kde.StatusNotifierItem";
/// 托盘 watcher 的总线服务名（KDE/GNOME 扩展都会暴露该协议）。
const WATCHER_SERVICE: &str = "org.kde.StatusNotifierWatcher";
/// 托盘 watcher 对象路径。
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
/// 托盘 watcher 接口名。
const WATCHER_INTERFACE: &str = "org.kde.StatusNotifierWatcher";
/// freedesktop 通知服务名。
const NOTIFICATIONS_SERVICE: &str = "org.freedesktop.Notifications";
/// freedesktop 通知对象路径。
const NOTIFICATIONS_PATH: &str = "/org/freedesktop/Notifications";
/// freedesktop 通知接口名。
const NOTIFICATIONS_INTERFACE: &str = "org.freedesktop.Notifications";

/// SNI 项对象路径。
const ITEM_PATH: &str = "/StatusNotifierItem";
/// DBusMenu 对象路径。
const MENU_PATH: &str = "/MenuBar";

/// 菜单布局版本号（dbusmenu 协议字段）。
const MENU_REVISION: u32 = 1;
/// 根节点 ID（dbusmenu 约定）。
const MENU_ROOT: i32 = 0;
/// 菜单项 ID：Open Tunnel。
const MENU_OPEN: i32 = 1;
/// 菜单项 ID：Close Tunnel。
const MENU_CLOSE: i32 = 2;
/// 菜单项 ID：Quit。
const MENU_QUIT: i32 = 3;
/// 正常通知默认展示时长。
const INFO_TIMEOUT_MS: i32 = 5_000;
/// 错误通知默认展示时长。
const ERROR_TIMEOUT_MS: i32 = 10_000;

/// dbusmenu 属性字典类型。
type MenuProps = HashMap<String, OwnedValue>;
/// dbusmenu 布局节点类型：(id, 属性, 子节点数组)。
type MenuLayout = (i32, MenuProps, Vec<OwnedValue>);

/// `org.kde.StatusNotifierItem` 接口实现。
#[derive(Clone)]
struct StatusNotifierItem {
    sender: Sender<TrayCommand>,
}

#[interface(name = "org.kde.StatusNotifierItem")]
impl StatusNotifierItem {
    /// 主激活事件（通常是左键点击托盘图标）。
    #[zbus(name = "Activate")]
    fn activate(&self, _x: i32, _y: i32) {
        let _ = self.sender.send(TrayCommand::ShowWindow);
    }

    /// 次级激活事件（某些宿主环境会触发）。
    #[zbus(name = "SecondaryActivate")]
    fn secondary_activate(&self, _x: i32, _y: i32) {
        let _ = self.sender.send(TrayCommand::ShowWindow);
    }

    /// 请求显示上下文菜单（菜单内容由 DBusMenu 接口提供）。
    #[zbus(name = "ContextMenu")]
    fn context_menu(&self, _x: i32, _y: i32) {}

    /// 滚轮事件（当前无需处理，保留空实现）。
    #[zbus(name = "Scroll")]
    fn scroll(&self, _delta: i32, _orientation: &str) {}

    /// 条目类别。
    #[zbus(property)]
    fn category(&self) -> &str {
        "ApplicationStatus"
    }

    /// 条目 ID（会在宿主环境中作为识别字段之一）。
    #[zbus(property)]
    fn id(&self) -> &str {
        "r-wg"
    }

    /// 条目标题。
    #[zbus(property)]
    fn title(&self) -> &str {
        "r-wg"
    }

    /// 当前状态（Active/Passive/NeedsAttention）。
    #[zbus(property)]
    fn status(&self) -> &str {
        "Active"
    }

    /// 托盘图标名称（走主题图标解析）。
    #[zbus(property)]
    fn icon_name(&self) -> &str {
        "network-vpn"
    }

    /// 是否把 item 自身当作菜单（false 表示使用单独 menu 对象）。
    #[zbus(property)]
    fn item_is_menu(&self) -> bool {
        false
    }

    /// 关联窗口 ID（Wayland/X11 通常可置 0）。
    #[zbus(property)]
    fn window_id(&self) -> i32 {
        0
    }

    /// 关联 DBusMenu 对象路径。
    #[zbus(property)]
    fn menu(&self) -> OwnedObjectPath {
        OwnedObjectPath::try_from(MENU_PATH).expect("menu path must be valid")
    }
}

/// `com.canonical.dbusmenu` 接口实现。
///
/// 用于给托盘宿主环境提供右键菜单结构与点击事件回调。
#[derive(Clone)]
struct DbusMenu {
    sender: Sender<TrayCommand>,
}

#[interface(name = "com.canonical.dbusmenu")]
impl DbusMenu {
    /// 返回菜单树布局。
    #[zbus(name = "GetLayout")]
    fn get_layout(
        &self,
        _parent_id: i32,
        _recursion_depth: i32,
        _property_names: Vec<String>,
    ) -> zbus::fdo::Result<(u32, MenuLayout)> {
        Ok((MENU_REVISION, root_layout()?))
    }

    /// 批量查询菜单项属性。
    #[zbus(name = "GetGroupProperties")]
    fn get_group_properties(
        &self,
        ids: Vec<i32>,
        _property_names: Vec<String>,
    ) -> Vec<(i32, MenuProps)> {
        ids.into_iter()
            .filter_map(|id| menu_props_for_id(id).map(|props| (id, props)))
            .collect()
    }

    /// 查询单个菜单项属性。
    #[zbus(name = "GetProperty")]
    fn get_property(&self, id: i32, name: &str) -> zbus::fdo::Result<OwnedValue> {
        let props = menu_props_for_id(id)
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs(format!("Unknown menu id: {id}")))?;
        props
            .get(name)
            .cloned()
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs(format!("Unknown property: {name}")))
    }

    /// 单个菜单事件（重点处理 `clicked`）。
    #[zbus(name = "Event")]
    fn event(&self, id: i32, event_id: &str, _data: OwnedValue, _timestamp: u32) {
        if event_id != "clicked" {
            return;
        }

        let command = match id {
            MENU_OPEN => Some(TrayCommand::StartTunnel),
            MENU_CLOSE => Some(TrayCommand::StopTunnel),
            MENU_QUIT => Some(TrayCommand::QuitApp),
            _ => None,
        };
        if let Some(command) = command {
            let _ = self.sender.send(command);
        }
    }

    /// 批量菜单事件。
    #[zbus(name = "EventGroup")]
    fn event_group(&self, events: Vec<(i32, String, OwnedValue, u32)>) -> Vec<i32> {
        let mut id_errors = Vec::new();
        for (id, event_id, data, timestamp) in events {
            if menu_props_for_id(id).is_none() {
                id_errors.push(id);
                continue;
            }
            self.event(id, &event_id, data, timestamp);
        }
        id_errors
    }

    /// 某节点即将展示时回调。
    #[zbus(name = "AboutToShow")]
    fn about_to_show(&self, id: i32) -> bool {
        id == MENU_ROOT
    }

    /// 批量“即将展示”回调。
    #[zbus(name = "AboutToShowGroup")]
    fn about_to_show_group(&self, ids: Vec<i32>) -> (Vec<i32>, Vec<i32>) {
        let id_errors = ids
            .iter()
            .copied()
            .filter(|id| menu_props_for_id(*id).is_none())
            .collect();
        (Vec::new(), id_errors)
    }

    /// dbusmenu 协议版本。
    #[zbus(property)]
    fn version(&self) -> u32 {
        4
    }

    /// 文本方向。
    #[zbus(property)]
    fn text_direction(&self) -> &str {
        "ltr"
    }

    /// 菜单整体状态。
    #[zbus(property)]
    fn status(&self) -> &str {
        "normal"
    }

    /// 图标主题路径（当前不额外指定）。
    #[zbus(property)]
    fn icon_theme_path(&self) -> Vec<String> {
        Vec::new()
    }
}

/// 启动 Linux 托盘线程，并在 2 秒内返回初始化结果。
///
/// 返回值：
/// - `true`：托盘服务注册成功；
/// - `false`：D-Bus 不可用或 watcher 不可达，应用降级为无托盘模式。
pub(super) fn spawn_tray_thread(sender: Sender<TrayCommand>) -> bool {
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let spawned = thread::Builder::new()
        .name("tray-linux".into())
        .spawn(move || {
            let tray = match LinuxTray::init(sender) {
                Some(tray) => {
                    let _ = ready_tx.send(true);
                    tray
                }
                None => {
                    let _ = ready_tx.send(false);
                    return;
                }
            };

            let _tray = tray;
            loop {
                thread::park_timeout(Duration::from_secs(3600));
            }
        });

    if spawned.is_err() {
        return false;
    }

    ready_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap_or(false)
}

/// Linux 下关闭窗口行为：最小化窗口。
pub(super) fn hide_window(window: &mut gpui::Window) {
    window.minimize_window();
}

/// Linux 下显示窗口行为：激活窗口使其回到前台。
pub(super) fn show_window(window: &mut gpui::Window) {
    window.activate_window();
}

/// Linux 下通过 freedesktop 通知规范发送桌面通知。
///
/// 兼容性说明：
/// - GNOME / KDE / Xfce / Cinnamon 等主流桌面通常都实现了该规范；
/// - 若当前会话没有通知服务，则静默降级，不影响隧道控制流程。
pub(super) fn notify_system(title: &str, message: &str, is_error: bool) {
    let connection = match Connection::session() {
        Ok(connection) => connection,
        Err(err) => {
            tracing::debug!("linux notification skipped: session bus unavailable: {err}");
            return;
        }
    };

    let icon = if is_error {
        "dialog-error"
    } else {
        "network-vpn"
    };
    let expire_timeout = if is_error {
        ERROR_TIMEOUT_MS
    } else {
        INFO_TIMEOUT_MS
    };
    let mut hints: HashMap<&str, OwnedValue> = HashMap::new();
    hints.insert("desktop-entry", owned_str("r-wg"));
    hints.insert("urgency", owned_u8(if is_error { 2 } else { 1 }));

    let result = connection.call_method(
        Some(NOTIFICATIONS_SERVICE),
        NOTIFICATIONS_PATH,
        Some(NOTIFICATIONS_INTERFACE),
        "Notify",
        &(
            "r-wg",
            0u32,
            icon,
            title,
            message,
            Vec::<String>::new(),
            hints,
            expire_timeout,
        ),
    );
    if let Err(err) = result {
        tracing::debug!("linux notification skipped: notify call failed: {err}");
    }
}

/// Linux 目前无需显式关闭托盘线程，保持 no-op。
pub(super) fn shutdown_tray() {}

/// 托盘运行时对象。
///
/// 保持 `Connection` 存活即可让对象服务持续可访问。
struct LinuxTray {
    _connection: Connection,
}

impl LinuxTray {
    /// 创建 SNI + DBusMenu 服务并注册到 watcher。
    fn init(sender: Sender<TrayCommand>) -> Option<Self> {
        let connection = Connection::session().ok()?;
        let service_name = format!("{ITEM_SERVICE_PREFIX}-{}-1", std::process::id());
        connection.request_name(service_name.as_str()).ok()?;

        connection
            .object_server()
            .at(
                ITEM_PATH,
                StatusNotifierItem {
                    sender: sender.clone(),
                },
            )
            .ok()?;
        connection
            .object_server()
            .at(MENU_PATH, DbusMenu { sender })
            .ok()?;

        if !wait_for_status_notifier(&connection, &service_name) {
            return None;
        }

        Some(Self {
            _connection: connection,
        })
    }
}

/// 向 watcher 注册当前托盘项。
///
/// 兼容性处理：
/// - 先尝试传 service name；
/// - 失败后回退传 object path（部分实现只接受其中一种）。
fn register_status_notifier_item(connection: &Connection, service_name: &str) -> bool {
    let registered_with_service = connection
        .call_method(
            Some(WATCHER_SERVICE),
            WATCHER_PATH,
            Some(WATCHER_INTERFACE),
            "RegisterStatusNotifierItem",
            &service_name,
        )
        .is_ok();
    if registered_with_service {
        return true;
    }

    connection
        .call_method(
            Some(WATCHER_SERVICE),
            WATCHER_PATH,
            Some(WATCHER_INTERFACE),
            "RegisterStatusNotifierItem",
            &ITEM_PATH,
        )
        .is_ok()
}

/// 等待 watcher 就绪并重试注册（最多 3 秒）。
///
/// 作用：降低桌面会话启动阶段 watcher 晚于应用启动导致的偶发失败。
fn wait_for_status_notifier(connection: &Connection, service_name: &str) -> bool {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if register_status_notifier_item(connection, service_name) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(200));
    }
}

/// 构建根菜单布局。
fn root_layout() -> zbus::fdo::Result<MenuLayout> {
    let children = vec![
        layout_variant(action_layout(MENU_OPEN, "Open Tunnel"))?,
        layout_variant(action_layout(MENU_CLOSE, "Close Tunnel"))?,
        layout_variant(action_layout(MENU_QUIT, "Quit"))?,
    ];
    Ok((MENU_ROOT, root_props(), children))
}

/// 构建叶子菜单项布局。
fn action_layout(id: i32, label: &str) -> MenuLayout {
    (id, action_props(label), Vec::new())
}

/// 将布局节点转换为 dbusmenu 需要的 `OwnedValue`。
fn layout_variant(layout: MenuLayout) -> zbus::fdo::Result<OwnedValue> {
    OwnedValue::try_from(Value::from(layout))
        .map_err(|err| zbus::fdo::Error::Failed(format!("Invalid menu layout: {err}")))
}

/// 根据菜单 ID 生成属性字典。
fn menu_props_for_id(id: i32) -> Option<MenuProps> {
    match id {
        MENU_ROOT => Some(root_props()),
        MENU_OPEN => Some(action_props("Open Tunnel")),
        MENU_CLOSE => Some(action_props("Close Tunnel")),
        MENU_QUIT => Some(action_props("Quit")),
        _ => None,
    }
}

/// 根节点属性。
fn root_props() -> MenuProps {
    let mut props = MenuProps::new();
    props.insert("label".into(), owned_str(""));
    props.insert("visible".into(), OwnedValue::from(true));
    props.insert("enabled".into(), OwnedValue::from(true));
    props.insert("children-display".into(), owned_str("submenu"));
    props
}

/// 标准动作菜单项属性。
fn action_props(label: &str) -> MenuProps {
    let mut props = MenuProps::new();
    props.insert("label".into(), owned_str(label));
    props.insert("visible".into(), OwnedValue::from(true));
    props.insert("enabled".into(), OwnedValue::from(true));
    props.insert("type".into(), owned_str("standard"));
    props
}

/// 将字符串包装为 D-Bus `OwnedValue`。
fn owned_str(value: &str) -> OwnedValue {
    OwnedValue::try_from(Value::from(value)).expect("dbus string must convert to value")
}

/// 将 `u8` 包装为 D-Bus `OwnedValue`。
fn owned_u8(value: u8) -> OwnedValue {
    OwnedValue::try_from(Value::from(value)).expect("dbus u8 must convert to value")
}
