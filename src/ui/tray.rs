use gpui::{AnyWindowHandle, App, Global, UpdateGlobal, Window};
use r_wg::backend::wg::{Engine, EngineError};
use std::sync::{mpsc, Arc, Mutex};

use super::state::WgApp;

/// 托盘线程向 UI 主循环发送的命令。
///
/// 设计目标：
/// - 平台层（Windows/Linux）只负责产生命令，不直接改 UI 状态；
/// - 业务层在 UI 线程里统一消费命令，避免跨线程操作 UI。
#[derive(Clone, Copy, Debug)]
pub enum TrayCommand {
    /// 显示并激活主窗口。
    ShowWindow,
    /// 启动隧道（对应菜单「Open Tunnel」）。
    StartTunnel,
    /// 停止隧道（对应菜单「Close Tunnel」）。
    StopTunnel,
    /// 退出应用（会先尝试停止隧道）。
    QuitApp,
}

/// 托盘可用状态（写入 GPUI 全局）。
///
/// `enabled = true` 代表托盘初始化成功，此时窗口关闭按钮应走“最小化到托盘”。
#[derive(Default)]
struct TrayState {
    enabled: bool,
}

impl Global for TrayState {}

/// 初始化托盘。
///
/// 行为说明：
/// - 仅在 Windows/Linux 尝试创建托盘线程；
/// - 创建成功后启动命令消费循环；
/// - 任一步失败都降级为“无托盘模式”，应用仍可正常使用。
pub fn init(
    window_handle: AnyWindowHandle,
    view: gpui::WeakEntity<WgApp>,
    engine: Engine,
    cx: &mut App,
) {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        let (tx, rx) = mpsc::channel();
        if platform::spawn_tray_thread(tx) {
            TrayState::set_global(cx, TrayState { enabled: true });
            start_command_loop(rx, window_handle, view, engine, cx);
            return;
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = (window_handle, &view, &engine);
    }

    TrayState::set_global(cx, TrayState { enabled: false });
}

/// 判断关闭窗口时是否应拦截为“最小化到托盘”。
pub fn should_minimize_on_close(cx: &App) -> bool {
    cx.try_global::<TrayState>()
        .map(|state| state.enabled)
        .unwrap_or(false)
}

/// 隐藏窗口（平台相关）。
///
/// - Windows：真正隐藏窗口；
/// - Linux：当前用最小化模拟隐藏（与 GPUI Linux 能力保持一致）。
pub fn hide_window(window: &mut Window) {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        platform::hide_window(window);
        return;
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        window.minimize_window();
    }
}

/// 显示窗口（平台相关）。
///
/// - Windows：显示窗口；
/// - Linux：通过激活窗口恢复到前台。
pub fn show_window(window: &mut Window) {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        platform::show_window(window);
        return;
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = window;
    }
}

/// 托盘命令消费循环。
///
/// 数据流：
/// 1. 托盘线程通过 `mpsc::Sender<TrayCommand>` 发送命令；
/// 2. 此循环在 UI 异步上下文中接收命令；
/// 3. 按命令分发到 UI 状态更新或后端引擎调用。
fn start_command_loop(
    rx: mpsc::Receiver<TrayCommand>,
    window_handle: AnyWindowHandle,
    view: gpui::WeakEntity<WgApp>,
    engine: Engine,
    cx: &mut App,
) {
    let rx = Arc::new(Mutex::new(rx));
    let view_handle = view.clone();
    let engine_handle = engine.clone();

    cx.spawn(async move |cx| loop {
        let rx = rx.clone();
        let cmd = cx
            .background_executor()
            .spawn(async move { rx.lock().ok()?.recv().ok() })
            .await;
        let Some(cmd) = cmd else { break };

        match cmd {
            TrayCommand::ShowWindow => {
                focus_main_window(window_handle, cx);
            }
            TrayCommand::StartTunnel => {
                let _ = view_handle.update(cx, |this, cx| {
                    this.handle_start_from_tray(cx);
                });
            }
            TrayCommand::StopTunnel => {
                let _ = view_handle.update(cx, |this, cx| {
                    this.handle_stop_from_tray(cx);
                });
            }
            TrayCommand::QuitApp => {
                request_quit(view_handle.clone(), engine_handle.clone(), cx).await;
            }
        }
    })
    .detach();
}

/// 将主窗口带回前台。
///
/// 说明：
/// - `show_window` 负责平台级显示动作；
/// - `activate_window` 负责焦点激活，提升“点托盘立即可见”的一致性。
fn focus_main_window(window_handle: AnyWindowHandle, cx: &mut gpui::AsyncApp) {
    let _ = window_handle.update(cx, |_, window, _| {
        show_window(window);
        window.activate_window();
    });
}

/// 处理“从托盘退出应用”。
///
/// 退出策略：
/// - 先尝试停止隧道；
/// - 对“已经停止/通道已关闭”视为可退出；
/// - 停止失败时保留应用并在 UI 显示错误。
async fn request_quit(view: gpui::WeakEntity<WgApp>, engine: Engine, cx: &mut gpui::AsyncApp) {
    let mut was_running = false;
    let _ = view.update(cx, |this, cx| {
        was_running = this.running;
        if this.running {
            this.busy = true;
            this.set_status("Stopping...");
            cx.notify();
        }
    });

    let result = cx
        .background_executor()
        .spawn(async move { engine.stop() })
        .await;
    let should_quit = matches!(
        &result,
        Ok(()) | Err(EngineError::NotRunning) | Err(EngineError::ChannelClosed)
    );

    let _ = view.update(cx, |this, cx| {
        if should_quit {
            if was_running {
                this.busy = false;
                this.running = false;
                this.running_name = None;
                this.running_id = None;
                this.started_at = None;
                this.clear_stats();
                this.set_status("Stopped");
            }
        } else if let Err(err) = result {
            if was_running {
                this.busy = false;
            }
            this.set_error(format!("Stop failed: {err}"));
        }
        cx.notify();
    });

    if should_quit {
        #[cfg(target_os = "windows")]
        platform::shutdown_tray();
        let _ = cx.update(|app| app.quit());
    }
}

#[cfg(target_os = "windows")]
mod platform {
    //! Windows 托盘实现（Win32 Shell_NotifyIcon + 隐藏消息窗口）。
    //!
    //! 架构：
    //! - 独立托盘线程创建隐藏窗口并注册托盘图标；
    //! - 左/右键事件在 `wnd_proc` 中解析后转成 `TrayCommand`；
    //! - 业务逻辑仍在上层 UI 线程执行。

    use super::TrayCommand;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::{
        ffi::OsStr,
        mem,
        os::windows::ffi::OsStrExt,
        sync::atomic::{AtomicIsize, Ordering},
        sync::mpsc::Sender,
        thread,
    };
    use windows::core::PCWSTR;
    use windows::Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM},
        Graphics::Gdi::HICON,
        System::LibraryLoader::GetModuleHandleW,
        UI::Shell::{
            Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
        },
        UI::WindowsAndMessaging::{
            AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
            DispatchMessageW, GetCursorPos, GetMessageW, GetWindowLongPtrW, LoadCursorW, LoadIconW,
            PostMessageW, PostQuitMessage, RegisterClassW, SetForegroundWindow, SetWindowLongPtrW,
            ShowWindow, TrackPopupMenu, CREATESTRUCTW, CW_USEDEFAULT, GWLP_USERDATA, HMENU,
            IDC_ARROW, IDI_APPLICATION, MF_SEPARATOR, MF_STRING, MSG, SW_HIDE, SW_SHOW,
            TPM_BOTTOMALIGN, TPM_RIGHTALIGN, TPM_RIGHTBUTTON, WM_APP, WM_CLOSE, WM_COMMAND,
            WM_DESTROY, WM_LBUTTONUP, WM_NCCREATE, WM_NULL, WM_RBUTTONUP, WM_USER, WNDCLASSW,
            WS_EX_NOACTIVATE, WS_OVERLAPPED,
        },
    };

    /// 托盘图标 ID（同一进程内保持唯一即可）。
    const TRAY_UID: u32 = 1;
    /// 托盘回调消息号（发送到隐藏窗口的自定义消息）。
    const WM_TRAYICON: u32 = WM_APP + 1;
    /// 菜单项命令 ID：启动隧道。
    const ID_START: usize = WM_USER as usize + 2;
    /// 菜单项命令 ID：停止隧道。
    const ID_STOP: usize = WM_USER as usize + 3;
    /// 菜单项命令 ID：退出应用。
    const ID_QUIT: usize = WM_USER as usize + 4;

    /// 托盘隐藏窗口句柄，用于退出时主动关闭托盘线程。
    static TRAY_HWND: AtomicIsize = AtomicIsize::new(0);

    /// 托盘窗口私有上下文，挂在 `GWLP_USERDATA`。
    struct TrayContext {
        /// 发给 UI 层的命令通道。
        sender: Sender<TrayCommand>,
        /// 右键弹出的菜单句柄。
        menu: HMENU,
        /// 托盘图标结构体（添加/删除图标都依赖它）。
        icon_data: NOTIFYICONDATAW,
    }

    /// 启动 Windows 托盘线程。
    pub(super) fn spawn_tray_thread(sender: Sender<TrayCommand>) -> bool {
        thread::Builder::new()
            .name("tray-thread".into())
            .spawn(move || unsafe {
                run_tray(sender);
            })
            .is_ok()
    }

    /// 隐藏主窗口（关闭按钮拦截后调用）。
    pub(super) fn hide_window(window: &mut gpui::Window) {
        with_hwnd(window, |hwnd| unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
        });
    }

    /// 显示主窗口（托盘点击“打开”时调用）。
    pub(super) fn show_window(window: &mut gpui::Window) {
        with_hwnd(window, |hwnd| unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
        });
    }

    /// 通知托盘线程退出（仅 Windows 需要显式清理托盘图标）。
    pub(super) fn shutdown_tray() {
        let hwnd = TRAY_HWND.load(Ordering::Acquire);
        if hwnd != 0 {
            unsafe {
                let _ = PostMessageW(HWND(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
    }

    /// 从 GPUI 窗口提取原生 `HWND` 并执行回调。
    fn with_hwnd(window: &gpui::Window, f: impl FnOnce(HWND)) {
        let Ok(handle) = window.window_handle() else {
            return;
        };
        if let RawWindowHandle::Win32(raw) = handle.as_raw() {
            let hwnd = HWND(raw.hwnd.get());
            f(hwnd);
        }
    }

    /// 托盘线程主函数。
    ///
    /// 步骤：
    /// 1. 注册窗口类；
    /// 2. 创建右键菜单；
    /// 3. 创建隐藏窗口并绑定上下文；
    /// 4. 添加托盘图标；
    /// 5. 进入消息循环。
    unsafe fn run_tray(sender: Sender<TrayCommand>) {
        let class_name = to_wide("r-wg-tray");
        let hinstance = GetModuleHandleW(None).unwrap_or_default();
        let wnd_class = WNDCLASSW {
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hInstance: hinstance.into(),
            lpszClassName: class_name.as_ptr().into(),
            lpfnWndProc: Some(wnd_proc),
            ..Default::default()
        };
        if RegisterClassW(&wnd_class) == 0 {
            return;
        }

        let menu = build_menu();
        if menu.0 == 0 {
            return;
        }

        // 准备托盘图标元数据（图标、提示文案、回调消息号）。
        let mut icon_data: NOTIFYICONDATAW = mem::zeroed();
        icon_data.cbSize = mem::size_of::<NOTIFYICONDATAW>() as u32;
        icon_data.uID = TRAY_UID;
        icon_data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        icon_data.uCallbackMessage = WM_TRAYICON;
        icon_data.hIcon = LoadIconW(None, IDI_APPLICATION).unwrap_or(HICON::default());
        set_tip(&mut icon_data, "r-wg");

        let context = Box::new(TrayContext {
            sender,
            menu,
            icon_data,
        });
        let context_ptr = Box::into_raw(context);

        // 创建隐藏窗口：不展示 UI，仅作为托盘事件接收端。
        let hwnd = CreateWindowExW(
            WS_EX_NOACTIVATE,
            class_name.as_ptr().into(),
            class_name.as_ptr().into(),
            WS_OVERLAPPED,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            HWND(0),
            None,
            hinstance,
            Some(context_ptr as *const _),
        );
        if hwnd.0 == 0 {
            drop(Box::from_raw(context_ptr));
            return;
        }

        // 将图标真正添加到系统托盘区域。
        TRAY_HWND.store(hwnd.0, Ordering::Release);
        let context_ref = &mut *context_ptr;
        context_ref.icon_data.hWnd = hwnd;
        let _ = Shell_NotifyIconW(NIM_ADD, &context_ref.icon_data);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, HWND(0), 0, 0).into() {
            DispatchMessageW(&msg);
        }
    }

    /// 托盘隐藏窗口消息处理函数。
    ///
    /// 关注三类消息：
    /// - `WM_TRAYICON`：图标点击；
    /// - `WM_COMMAND`：菜单项点击；
    /// - `WM_DESTROY`：清理图标与菜单资源。
    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if msg == WM_NCCREATE {
            let createstruct = lparam.0 as *const CREATESTRUCTW;
            let context = (*createstruct).lpCreateParams as isize;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, context);
            return LRESULT(1);
        }

        let context_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut TrayContext;
        let context = context_ptr.as_mut();

        match msg {
            WM_TRAYICON => {
                if let Some(ctx) = context {
                    match lparam.0 as u32 {
                        // 左键：恢复主窗口。
                        WM_LBUTTONUP => {
                            let _ = ctx.sender.send(TrayCommand::ShowWindow);
                        }
                        // 右键：弹出操作菜单。
                        WM_RBUTTONUP => {
                            show_menu(hwnd, ctx.menu);
                        }
                        _ => {}
                    }
                }
                LRESULT(0)
            }
            WM_COMMAND => {
                if let Some(ctx) = context {
                    // 低 16 位即菜单 ID。
                    let id = (wparam.0 & 0xffff) as usize;
                    let command = match id {
                        ID_START => Some(TrayCommand::StartTunnel),
                        ID_STOP => Some(TrayCommand::StopTunnel),
                        ID_QUIT => Some(TrayCommand::QuitApp),
                        _ => None,
                    };
                    if let Some(command) = command {
                        let _ = ctx.sender.send(command);
                    }
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                if let Some(ctx) = context {
                    // 删除托盘图标并释放菜单/上下文。
                    let _ = Shell_NotifyIconW(NIM_DELETE, &ctx.icon_data);
                    let _ = DestroyMenu(ctx.menu);
                    drop(Box::from_raw(context_ptr));
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                }
                TRAY_HWND.store(0, Ordering::Release);
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }

    /// 在鼠标位置弹出托盘右键菜单。
    unsafe fn show_menu(hwnd: HWND, menu: HMENU) {
        let mut point = POINT::default();
        if GetCursorPos(&mut point).as_bool() {
            SetForegroundWindow(hwnd);
            let _ = TrackPopupMenu(
                menu,
                TPM_RIGHTBUTTON | TPM_RIGHTALIGN | TPM_BOTTOMALIGN,
                point.x,
                point.y,
                0,
                hwnd,
                None,
            );
            let _ = PostMessageW(hwnd, WM_NULL, WPARAM(0), LPARAM(0));
        }
    }

    /// 构建托盘右键菜单。
    ///
    /// 菜单文案与用户需求保持一致：
    /// - Open Tunnel
    /// - Close Tunnel
    fn build_menu() -> HMENU {
        unsafe {
            let menu = CreatePopupMenu();
            if menu.0 == 0 {
                return menu;
            }

            let _ = AppendMenuW(
                menu,
                MF_STRING,
                ID_START,
                PCWSTR(to_wide("Open Tunnel").as_ptr()),
            );
            let _ = AppendMenuW(
                menu,
                MF_STRING,
                ID_STOP,
                PCWSTR(to_wide("Close Tunnel").as_ptr()),
            );
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(menu, MF_STRING, ID_QUIT, PCWSTR(to_wide("Quit").as_ptr()));
            menu
        }
    }

    /// 设置托盘图标提示文案（NUL 结尾 UTF-16）。
    fn set_tip(icon: &mut NOTIFYICONDATAW, tip: &str) {
        let wide = to_wide(tip);
        let max = icon.szTip.len().saturating_sub(1);
        let count = wide.len().saturating_sub(1).min(max);
        icon.szTip[..count].copy_from_slice(&wide[..count]);
        icon.szTip[count] = 0;
    }

    /// UTF-8 字符串转 UTF-16（Win32 API 入参格式）。
    fn to_wide(text: &str) -> Vec<u16> {
        OsStr::new(text).encode_wide().chain(Some(0)).collect()
    }
}

#[cfg(target_os = "linux")]
mod platform {
    //! Linux 托盘实现（StatusNotifierItem + DBusMenu）。
    //!
    //! 兼容性说明：
    //! - KDE：原生支持 SNI（Status Notifier Item）；
    //! - GNOME：需启用 AppIndicator/StatusNotifier 类扩展后可显示托盘图标。
    //!
    //! 本模块只负责 D-Bus 协议适配，把点击动作转换成 `TrayCommand` 回传上层。

    use super::TrayCommand;
    use std::collections::HashMap;
    use std::sync::mpsc::{self, Sender};
    use std::thread;
    use std::time::{Duration, Instant};
    use zbus::blocking::Connection;
    use zbus::interface;
    use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

    /// SNI 服务名前缀，最终会拼上 PID，避免同名冲突。
    const ITEM_SERVICE_PREFIX: &str = "org.kde.StatusNotifierItem";
    /// 托盘 watcher 的总线服务名（KDE/GNOME 扩展都会暴露该协议）。
    const WATCHER_SERVICE: &str = "org.kde.StatusNotifierWatcher";
    /// 托盘 watcher 对象路径。
    const WATCHER_PATH: &str = "/StatusNotifierWatcher";
    /// 托盘 watcher 接口名。
    const WATCHER_INTERFACE: &str = "org.kde.StatusNotifierWatcher";

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
}
