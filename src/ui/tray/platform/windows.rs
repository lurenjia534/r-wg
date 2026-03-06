//! Windows 托盘实现（Win32 Shell_NotifyIcon + 隐藏消息窗口）。
//!
//! 架构：
//! - 独立托盘线程创建隐藏窗口并注册托盘图标；
//! - 左/右键事件在 `wnd_proc` 中解析后转成 `TrayCommand`；
//! - UI 状态更新与退出编排仍由上层 controller 负责。

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
    System::LibraryLoader::GetModuleHandleW,
    UI::Shell::{
        Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_ERROR, NIIF_INFO,
        NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
    },
    UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
        DispatchMessageW, GetCursorPos, GetMessageW, GetWindowLongPtrW, LoadCursorW, LoadIconW,
        PostMessageW, PostQuitMessage, RegisterClassW, SetForegroundWindow, SetWindowLongPtrW,
        ShowWindow, TrackPopupMenu, CREATESTRUCTW, CW_USEDEFAULT, GWLP_USERDATA, HICON, HMENU,
        IDC_ARROW, IDI_APPLICATION, MF_SEPARATOR, MF_STRING, MSG, SW_HIDE, SW_SHOW,
        TPM_BOTTOMALIGN, TPM_RIGHTALIGN, TPM_RIGHTBUTTON, WM_APP, WM_CLOSE, WM_COMMAND, WM_DESTROY,
        WM_LBUTTONUP, WM_NCCREATE, WM_NULL, WM_RBUTTONUP, WM_USER, WNDCLASSW, WS_EX_NOACTIVATE,
        WS_OVERLAPPED,
    },
};

use crate::ui::tray::types::TrayCommand;

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
            let _ = PostMessageW(Some(HWND(hwnd as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}

/// 通过托盘图标发送一条系统通知。
///
/// 实现要点：
/// - 复用已注册的托盘图标（`TRAY_UID`），避免再创建新窗口或新进程；
/// - 使用 `NIM_MODIFY + NIF_INFO` 让系统展示气泡/通知中心消息；
/// - `is_error=true` 时使用错误图标，便于用户区分失败事件。
pub(super) fn notify_system(title: &str, message: &str, is_error: bool) {
    let hwnd = TRAY_HWND.load(Ordering::Acquire);
    if hwnd == 0 {
        // 托盘尚未初始化（或已退出）时直接跳过，不影响主流程。
        return;
    }

    unsafe {
        let mut icon_data: NOTIFYICONDATAW = mem::zeroed();
        icon_data.cbSize = mem::size_of::<NOTIFYICONDATAW>() as u32;
        icon_data.hWnd = HWND(hwnd as *mut _);
        icon_data.uID = TRAY_UID;
        icon_data.uFlags = NIF_INFO;
        icon_data.dwInfoFlags = if is_error { NIIF_ERROR } else { NIIF_INFO };
        set_text_field(&mut icon_data.szInfoTitle, title);
        set_text_field(&mut icon_data.szInfo, message);

        let _ = Shell_NotifyIconW(NIM_MODIFY, &icon_data);
    }
}

/// 从 GPUI 窗口提取原生 `HWND` 并执行回调。
fn with_hwnd(window: &gpui::Window, f: impl FnOnce(HWND)) {
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    if let RawWindowHandle::Win32(raw) = handle.as_raw() {
        let hwnd = HWND(raw.hwnd.get() as *mut _);
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

    // 将图标真正添加到系统托盘区域。
    TRAY_HWND.store(hwnd.0 as isize, Ordering::Release);
    let context_ref = &mut *context_ptr;
    context_ref.icon_data.hWnd = hwnd;
    let _ = Shell_NotifyIconW(NIM_ADD, &context_ref.icon_data);

    let mut msg = MSG::default();
    while GetMessageW(&mut msg, None, 0, 0).into() {
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
/// 菜单文案与用户需求保持一致：
/// - Open Tunnel
/// - Close Tunnel
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

/// 设置托盘图标提示文案（NUL 结尾 UTF-16）。
fn set_tip(icon: &mut NOTIFYICONDATAW, tip: &str) {
    let wide = to_wide(tip);
    let max = icon.szTip.len().saturating_sub(1);
    let count = wide.len().saturating_sub(1).min(max);
    icon.szTip[..count].copy_from_slice(&wide[..count]);
    icon.szTip[count] = 0;
}

/// 写入定长 UTF-16 字段（自动 NUL 结尾并在超长时截断）。
///
/// `NOTIFYICONDATAW` 的标题/正文是固定长度数组，必须手动处理截断和终止符。
fn set_text_field(field: &mut [u16], text: &str) {
    let wide = to_wide(text);
    let max = field.len().saturating_sub(1);
    let count = wide.len().saturating_sub(1).min(max);
    field[..count].copy_from_slice(&wide[..count]);
    field[count] = 0;
}

/// UTF-8 字符串转 UTF-16（Win32 API 入参格式）。
fn to_wide(text: &str) -> Vec<u16> {
    OsStr::new(text).encode_wide().chain(Some(0)).collect()
}
