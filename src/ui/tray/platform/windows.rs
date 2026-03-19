//! Windows 托盘平台实现（Win32 `Shell_NotifyIconW` + 隐藏消息窗口 + 通知复制兜底）。
//!
//! 当前模块负责三类 Windows 专属能力：
//! - 在独立托盘线程中创建隐藏窗口、注册托盘图标并处理 Win32 消息循环；
//! - 把托盘左键、右键、菜单命令等平台事件转换成上层可消费的 `TrayCommand`；
//! - 在沿用旧式托盘气泡通知的前提下，补上“最近通知可复制”的能力。
//!
//! 设计背景：
//! - 现有实现使用的是 `Shell_NotifyIconW(..., NIF_INFO)` 这条 Win32 路径，而不是 WinRT Toast；
//! - 这条路径与当前应用架构兼容性更好，不依赖 AUMID、快捷方式注册或打包分发前提；
//! - 但系统展示出的通知文本本身不可直接选中复制，这对隧道启动/关闭失败时的错误排查不够友好。
//!
//! 因此本模块额外维护“最近一条通知正文”缓存，并提供两条复制路径：
//! - 用户点击通知气泡时，直接把最近通知写入系统剪贴板；
//! - 用户从托盘菜单选择 `Copy Last Notification` 时，也可重新复制最近通知原文。
//!
//! 职责边界：
//! - 平台层只负责托盘、通知、剪贴板和 Win32 消息处理；
//! - UI 状态更新、窗口激活、退出编排仍由上层 controller 统一调度；
//! - 这样可以保持平台代码聚焦于系统交互，避免把业务状态机耦合进 `wnd_proc`。
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    ffi::OsStr,
    mem,
    os::windows::ffi::OsStrExt,
    ptr,
    sync::{
        atomic::{AtomicIsize, Ordering},
        mpsc::Sender,
        Mutex, OnceLock,
    },
    thread,
};
use windows::core::PCWSTR;
use windows::Win32::{
    Foundation::{GlobalFree, HANDLE, HWND, LPARAM, LRESULT, POINT, WPARAM},
    System::{
        DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData},
        LibraryLoader::GetModuleHandleW,
        Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE},
        Ole::CF_UNICODETEXT,
    },
    UI::Shell::{
        Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_ERROR, NIIF_INFO,
        NIM_ADD, NIM_DELETE, NIM_MODIFY, NIN_BALLOONUSERCLICK, NOTIFYICONDATAW,
    },
    UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
        DispatchMessageW, GetCursorPos, GetMessageW, GetWindowLongPtrW, LoadCursorW, LoadIconW,
        LoadImageW, PostMessageW, PostQuitMessage, RegisterClassW, SetForegroundWindow,
        SetWindowLongPtrW, ShowWindow, TrackPopupMenu, CREATESTRUCTW, CW_USEDEFAULT, GWLP_USERDATA,
        HICON, HMENU, IDC_ARROW, IDI_APPLICATION, IMAGE_ICON, LR_DEFAULTSIZE, LR_SHARED,
        MF_SEPARATOR, MF_STRING, MSG, SW_HIDE, SW_SHOW, TPM_BOTTOMALIGN, TPM_RIGHTALIGN,
        TPM_RIGHTBUTTON, WM_APP, WM_CLOSE, WM_COMMAND, WM_DESTROY, WM_LBUTTONUP, WM_NCCREATE,
        WM_NULL, WM_RBUTTONUP, WM_USER, WNDCLASSW, WS_EX_NOACTIVATE, WS_OVERLAPPED,
    },
};

use crate::ui::tray::types::TrayCommand;

/// 托盘图标 ID。
///
/// 同一进程里只要保持唯一即可；后续修改、删除托盘图标时
/// 都必须使用同一个 `uID` 才能命中同一个图标实例。
const TRAY_UID: u32 = 1;
/// 托盘回调消息号。
///
/// Shell 会把托盘图标左键、右键、气泡点击等交互统一转发到这个消息，
/// 再由 `wnd_proc` 根据 `lparam` 识别具体事件类型。
const WM_TRAYICON: u32 = WM_APP + 1;
/// 菜单项命令 ID：启动隧道。
const ID_START: usize = WM_USER as usize + 2;
/// 菜单项命令 ID：停止隧道。
const ID_STOP: usize = WM_USER as usize + 3;
/// 菜单项命令 ID：复制最近一条通知。
///
/// 这是修复“Windows 系统通知内容不可复制”的关键入口之一。
/// 旧式 `Shell_NotifyIconW(NIF_INFO)` 气泡只能显示文本，不能像普通文本控件那样选中复制，
/// 因此我们增加一个显式复制动作，把最近发送的通知原文写入系统剪贴板。
const ID_COPY_NOTIFICATION: usize = WM_USER as usize + 4;
/// 菜单项命令 ID：退出应用。
const ID_QUIT: usize = WM_USER as usize + 5;

/// 托盘隐藏窗口句柄。
///
/// Windows 托盘线程拥有自己的消息循环；退出时需要主动给这个隐藏窗口发 `WM_CLOSE`，
/// 让它在自己的线程里清理托盘图标和菜单资源，所以这里把句柄存成全局原子变量。
static TRAY_HWND: AtomicIsize = AtomicIsize::new(0);
/// 最近一条系统通知的缓存。
///
/// 根因在于当前实现使用的不是 WinRT Toast，而是老式 `Shell_NotifyIconW + NIF_INFO`。
/// 这条 Win32 路径与现有架构兼容性很好，但气泡文本本身不支持直接复制。
/// 因此每次发通知时都把原始正文缓存起来，后续可通过“点击气泡”或“托盘菜单”完成复制。
static LAST_NOTIFICATION: OnceLock<Mutex<Option<NotificationPayload>>> = OnceLock::new();

/// 最近通知的缓存载体。
///
/// 目前只保存正文，因为启动/停止失败时用户真正需要复制的是错误消息本身。
/// 如果后续需要连标题一并复制，可以在这里继续扩展字段。
#[derive(Clone)]
struct NotificationPayload {
    message: String,
}

/// 托盘线程私有上下文。
///
/// 这份上下文通过 `GWLP_USERDATA` 挂到隐藏窗口上，
/// 之后 `wnd_proc` 处理每一条消息时都可以把它取回来。
struct TrayContext {
    /// 发回 UI 主线程的托盘命令通道。
    sender: Sender<TrayCommand>,
    /// 托盘右键菜单句柄。
    menu: HMENU,
    /// 已注册托盘图标的元数据。
    ///
    /// 删除图标时也要把同一份数据再传回 Shell，
    /// 所以把它保存在上下文里复用。
    icon_data: NOTIFYICONDATAW,
}

/// 启动 Windows 托盘线程。
///
/// 托盘逻辑和 GPUI 主线程分离：
/// - Win32 消息循环留在托盘线程；
/// - UI 状态更新通过 `TrayCommand` 回到主线程；
/// 这样职责更清晰，也避免跨线程直接操作 UI。
pub(super) fn spawn_tray_thread(sender: Sender<TrayCommand>) -> bool {
    thread::Builder::new()
        .name("tray-thread".into())
        .spawn(move || unsafe {
            run_tray(sender);
        })
        .is_ok()
}

/// 隐藏主窗口。
///
/// 在“关闭按钮最小化到托盘”的语义下，真正执行的不是退出应用，
/// 而是把窗口隐藏起来。
pub(super) fn hide_window(window: &mut gpui::Window) {
    with_hwnd(window, |hwnd| unsafe {
        let _ = ShowWindow(hwnd, SW_HIDE);
    });
}

/// 显示主窗口。
///
/// 托盘左键点击图标恢复窗口时会走这里。
pub(super) fn show_window(window: &mut gpui::Window) {
    with_hwnd(window, |hwnd| unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
    });
}

/// 请求关闭托盘线程。
///
/// 这里不能只改某个布尔状态，因为 Windows 托盘图标需要显式删除；
/// 所以通过给隐藏窗口发送 `WM_CLOSE`，让它在正确的线程内完成清理。
pub(super) fn shutdown_tray() {
    let hwnd = TRAY_HWND.load(Ordering::Acquire);
    if hwnd != 0 {
        unsafe {
            let _ = PostMessageW(Some(HWND(hwnd as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}

/// 发送一条系统通知。
///
/// 当前 Windows 平台仍然沿用 `Shell_NotifyIconW + NIF_INFO`：
/// - 优点是与现有托盘架构兼容，不要求 AUMID、开始菜单快捷方式、AppX/MSIX 打包等前提；
/// - 缺点是系统弹出的通知内容不可直接选中复制。
///
/// 因此这里在真正展示通知前，先把正文缓存到 `LAST_NOTIFICATION`，
/// 后续不论用户点击气泡还是点托盘菜单中的“Copy Last Notification”，
/// 都能拿到同一条原始消息文本。
pub(super) fn notify_system(title: &str, message: &str, is_error: bool) {
    let hwnd = TRAY_HWND.load(Ordering::Acquire);
    if hwnd == 0 {
        return;
    }

    remember_notification(message);
    unsafe {
        show_balloon(HWND(hwnd as *mut _), title, message, is_error);
    }
}

/// 从 GPUI 窗口提取底层 `HWND`。
///
/// 平台相关的 Win32 API 仍然要求原生窗口句柄，
/// 这里负责把跨平台 `gpui::Window` 下钻成 Windows 句柄再交给回调处理。
fn with_hwnd(window: &gpui::Window, f: impl FnOnce(HWND)) {
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    if let RawWindowHandle::Win32(raw) = handle.as_raw() {
        let hwnd = HWND(raw.hwnd.get() as *mut _);
        f(hwnd);
    }
}

/// 读取 EXE 内嵌的应用图标，和主窗口/任务栏保持一致。
fn load_tray_icon() -> HICON {
    let module = unsafe { GetModuleHandleW(None) }.unwrap_or_default();
    let handle = unsafe {
        LoadImageW(
            Some(module.into()),
            PCWSTR(1 as _),
            IMAGE_ICON,
            0,
            0,
            LR_DEFAULTSIZE | LR_SHARED,
        )
    };
    handle.map(|icon| HICON(icon.0)).unwrap_or_else(|_| {
        // SAFETY: `None` asks Windows for a shared predefined system icon,
        // and `IDI_APPLICATION` is a valid predefined icon resource id.
        unsafe { LoadIconW(None, IDI_APPLICATION).unwrap_or_default() }
    })
}

/// 托盘线程主函数。
///
/// 流程如下：
/// 1. 注册隐藏窗口类；
/// 2. 创建右键菜单；
/// 3. 创建隐藏窗口并绑定上下文；
/// 4. 把图标注册到系统托盘；
/// 5. 进入 Win32 消息循环，等待 Shell 回调。
unsafe fn run_tray(sender: Sender<TrayCommand>) {
    let class_name = to_wide("r-wg-tray");
    let hinstance = GetModuleHandleW(None).unwrap_or_default();
    let wnd_class = WNDCLASSW {
        hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
        hInstance: hinstance.into(),
        lpszClassName: PCWSTR::from_raw(class_name.as_ptr()),
        lpfnWndProc: Some(wnd_proc),
        ..Default::default()
    };
    if RegisterClassW(&wnd_class) == 0 {
        return;
    }

    let menu = build_menu();
    if menu.0.is_null() {
        return;
    }

    // 准备托盘图标元数据：
    // - `NIF_MESSAGE`：让 Shell 把托盘交互事件回送到 `WM_TRAYICON`
    // - `NIF_ICON`：提供托盘图标
    // - `NIF_TIP`：提供鼠标悬停提示
    let mut icon_data: NOTIFYICONDATAW = mem::zeroed();
    icon_data.cbSize = mem::size_of::<NOTIFYICONDATAW>() as u32;
    icon_data.uID = TRAY_UID;
    icon_data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    icon_data.uCallbackMessage = WM_TRAYICON;
    icon_data.hIcon = load_tray_icon();
    set_tip(&mut icon_data, "r-wg");

    let context = Box::new(TrayContext {
        sender,
        menu,
        icon_data,
    });
    let context_ptr = Box::into_raw(context);

    // 创建一个不可激活的隐藏窗口，专门承接托盘消息，
    // 不参与应用正常可见 UI 的显示。
    let hwnd = CreateWindowExW(
        WS_EX_NOACTIVATE,
        PCWSTR::from_raw(class_name.as_ptr()),
        PCWSTR::from_raw(class_name.as_ptr()),
        WS_OVERLAPPED,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        None,
        None,
        Some(hinstance.into()),
        Some(context_ptr as *const _),
    )
    .unwrap_or_default();
    if hwnd.0.is_null() {
        drop(Box::from_raw(context_ptr));
        return;
    }

    // 隐藏窗口就绪后，把托盘图标注册给系统。
    TRAY_HWND.store(hwnd.0 as isize, Ordering::Release);
    let context_ref = &mut *context_ptr;
    context_ref.icon_data.hWnd = hwnd;
    let _ = Shell_NotifyIconW(NIM_ADD, &context_ref.icon_data);

    let mut msg = MSG::default();
    while GetMessageW(&mut msg, None, 0, 0).into() {
        DispatchMessageW(&msg);
    }
}

/// 托盘隐藏窗口的消息处理函数。
///
/// 主要处理三类消息：
/// - `WM_TRAYICON`：托盘图标点击与通知气泡点击；
/// - `WM_COMMAND`：右键菜单项命令；
/// - `WM_DESTROY`：托盘资源清理。
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
                    // 左键托盘图标：恢复主窗口。
                    WM_LBUTTONUP => {
                        let _ = ctx.sender.send(TrayCommand::ShowWindow);
                    }
                    // 右键托盘图标：弹出托盘菜单。
                    WM_RBUTTONUP => {
                        show_menu(hwnd, ctx.menu);
                    }
                    // 点击通知气泡：把最近一条通知直接复制到剪贴板。
                    //
                    // 这里的目标不是让系统通知本身“变成可选中文本”，
                    // 而是提供一个更贴近用户直觉的复制动作。
                    NIN_BALLOONUSERCLICK => {
                        copy_latest_notification(hwnd);
                    }
                    _ => {}
                }
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            if let Some(ctx) = context {
                // 菜单命令 ID 位于 `wparam` 的低 16 位。
                let id = (wparam.0 & 0xffff) as usize;
                match id {
                    // 托盘菜单里的显式复制入口。
                    ID_COPY_NOTIFICATION => copy_latest_notification(hwnd),
                    ID_START => {
                        let _ = ctx.sender.send(TrayCommand::StartTunnel);
                    }
                    ID_STOP => {
                        let _ = ctx.sender.send(TrayCommand::StopTunnel);
                    }
                    ID_QUIT => {
                        let _ = ctx.sender.send(TrayCommand::QuitApp);
                    }
                    _ => {}
                }
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            if let Some(ctx) = context {
                // 退出时必须显式删除托盘图标，否则系统托盘里可能残留僵尸图标。
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

/// 在鼠标当前位置弹出托盘右键菜单。
///
/// `SetForegroundWindow + TrackPopupMenu + WM_NULL` 是常见的 Win32 托盘菜单调用顺序，
/// 用来确保菜单能正确获取焦点并在交互结束后正常收起。
unsafe fn show_menu(hwnd: HWND, menu: HMENU) {
    let mut point = POINT::default();
    if GetCursorPos(&mut point).is_ok() {
        let _ = SetForegroundWindow(hwnd);
        let _ = TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON | TPM_RIGHTALIGN | TPM_BOTTOMALIGN,
            point.x,
            point.y,
            None,
            hwnd,
            None,
        );
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
    }
}

/// 构建托盘右键菜单。
///
/// 新增的 `Copy Last Notification` 不依赖通知还停留在屏幕上；
/// 只要本进程内最近发过一条系统通知，就可以重新复制那条原文。
fn build_menu() -> HMENU {
    unsafe {
        let menu = CreatePopupMenu().unwrap_or_default();
        if menu.0.is_null() {
            return menu;
        }

        let _ = AppendMenuW(
            menu,
            MF_STRING,
            ID_START,
            PCWSTR::from_raw(to_wide("Open Tunnel").as_ptr()),
        );
        let _ = AppendMenuW(
            menu,
            MF_STRING,
            ID_STOP,
            PCWSTR::from_raw(to_wide("Close Tunnel").as_ptr()),
        );
        let _ = AppendMenuW(
            menu,
            MF_STRING,
            ID_COPY_NOTIFICATION,
            PCWSTR::from_raw(to_wide("Copy Last Notification").as_ptr()),
        );
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(
            menu,
            MF_STRING,
            ID_QUIT,
            PCWSTR::from_raw(to_wide("Quit").as_ptr()),
        );
        menu
    }
}

/// 设置托盘图标悬停提示。
///
/// `NOTIFYICONDATAW` 的字符串字段是固定长度 UTF-16 数组，
/// 这里统一负责截断和补 NUL 终止符。
fn set_tip(icon: &mut NOTIFYICONDATAW, tip: &str) {
    let wide = to_wide(tip);
    let max = icon.szTip.len().saturating_sub(1);
    let count = wide.len().saturating_sub(1).min(max);
    icon.szTip[..count].copy_from_slice(&wide[..count]);
    icon.szTip[count] = 0;
}

/// 写入 Win32 固定长度 UTF-16 字段。
///
/// `NOTIFYICONDATAW.szInfoTitle` 与 `szInfo` 都是定长数组，
/// 因此不能直接塞入任意长度字符串，必须手动截断并补上终止符。
fn set_text_field(field: &mut [u16], text: &str) {
    let wide = to_wide(text);
    let max = field.len().saturating_sub(1);
    let count = wide.len().saturating_sub(1).min(max);
    field[..count].copy_from_slice(&wide[..count]);
    field[count] = 0;
}

/// 获取最近通知的缓存槽。
///
/// 这里使用 `OnceLock + Mutex`：
/// - 只需惰性初始化一次；
/// - 数据量很小；
/// - 托盘线程和触发通知的线程之间需要安全共享。
fn notification_store() -> &'static Mutex<Option<NotificationPayload>> {
    LAST_NOTIFICATION.get_or_init(|| Mutex::new(None))
}

/// 缓存最近一条通知正文。
///
/// 每次发通知都覆盖旧值，语义上等价于“复制最近一条通知”。
fn remember_notification(message: &str) {
    if let Ok(mut slot) = notification_store().lock() {
        *slot = Some(NotificationPayload {
            message: message.to_string(),
        });
    }
}

/// 读取最近一条通知正文。
///
/// 返回克隆后的 `String`，避免在后续剪贴板写入过程中持有互斥锁。
fn latest_notification_text() -> Option<String> {
    notification_store()
        .lock()
        .ok()
        .and_then(|slot| slot.as_ref().map(|item| item.message.clone()))
}

/// 通过托盘图标显示气泡通知。
///
/// 这里故意不更新最近通知缓存，原因是这条函数也被用于“复制成功/失败”的反馈提示；
/// 如果在这里统一覆盖缓存，反而会把真正需要复制的错误原文替换掉。
unsafe fn show_balloon(hwnd: HWND, title: &str, message: &str, is_error: bool) {
    let mut icon_data: NOTIFYICONDATAW = mem::zeroed();
    icon_data.cbSize = mem::size_of::<NOTIFYICONDATAW>() as u32;
    icon_data.hWnd = hwnd;
    icon_data.uID = TRAY_UID;
    icon_data.uFlags = NIF_INFO;
    icon_data.dwInfoFlags = if is_error { NIIF_ERROR } else { NIIF_INFO };
    set_text_field(&mut icon_data.szInfoTitle, title);
    set_text_field(&mut icon_data.szInfo, message);

    let _ = Shell_NotifyIconW(NIM_MODIFY, &icon_data);
}

/// 把最近一条通知复制到系统剪贴板。
///
/// 这里同时服务两个交互入口：
/// - 用户点击气泡通知；
/// - 用户从托盘菜单选择 `Copy Last Notification`。
unsafe fn copy_latest_notification(hwnd: HWND) {
    let Some(text) = latest_notification_text() else {
        show_balloon(hwnd, "r-wg", "No notification available to copy", true);
        return;
    };

    match copy_text_to_clipboard(hwnd, &text) {
        Ok(()) => show_balloon(hwnd, "r-wg", "Notification copied to clipboard", false),
        Err(()) => show_balloon(hwnd, "r-wg", "Failed to copy notification", true),
    }
}

/// 把 UTF-16 文本写入 Windows 剪贴板。
///
/// 关键步骤：
/// 1. 打开剪贴板；
/// 2. 申请 `GMEM_MOVEABLE` 全局内存；
/// 3. 写入 NUL 结尾 UTF-16 文本；
/// 4. 清空旧剪贴板内容；
/// 5. 以 `CF_UNICODETEXT` 格式移交给系统。
///
/// 注意：`SetClipboardData` 成功后，内存所有权转移给系统，
/// 这时不能再由进程自己释放那块内存。
unsafe fn copy_text_to_clipboard(hwnd: HWND, text: &str) -> Result<(), ()> {
    OpenClipboard(Some(hwnd)).map_err(|_| ())?;

    let wide = to_wide(text);
    let bytes = wide.len() * mem::size_of::<u16>();
    let handle = GlobalAlloc(GMEM_MOVEABLE, bytes).map_err(|_| {
        let _ = CloseClipboard();
        ()
    })?;

    // `GlobalLock` 返回可写指针，把完整 UTF-16 内容拷贝到共享内存里。
    let memory = GlobalLock(handle) as *mut u16;
    if memory.is_null() {
        let _ = GlobalFree(Some(handle));
        let _ = CloseClipboard();
        return Err(());
    }

    ptr::copy_nonoverlapping(wide.as_ptr(), memory, wide.len());
    let _ = GlobalUnlock(handle);

    // 先清空旧剪贴板内容，再写入新的 Unicode 文本。
    if EmptyClipboard().is_err() {
        let _ = GlobalFree(Some(handle));
        let _ = CloseClipboard();
        return Err(());
    }

    if SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(handle.0))).is_err() {
        let _ = GlobalFree(Some(handle));
        let _ = CloseClipboard();
        return Err(());
    }

    CloseClipboard().map_err(|_| ())?;
    Ok(())
}

/// 把 Rust UTF-8 字符串转换成 Win32 常用的 NUL 结尾 UTF-16 文本。
fn to_wide(text: &str) -> Vec<u16> {
    OsStr::new(text).encode_wide().chain(Some(0)).collect()
}
