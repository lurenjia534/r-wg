#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "windows")]
mod windows;

use gpui::Window;
use std::sync::mpsc::Sender;

use super::types::TrayCommand;

#[cfg(target_os = "windows")]
pub(super) fn spawn_tray_thread(sender: Sender<TrayCommand>) -> bool {
    windows::spawn_tray_thread(sender)
}

#[cfg(target_os = "linux")]
pub(super) fn spawn_tray_thread(sender: Sender<TrayCommand>) -> bool {
    linux::spawn_tray_thread(sender)
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub(super) fn spawn_tray_thread(_sender: Sender<TrayCommand>) -> bool {
    false
}

#[cfg(target_os = "windows")]
pub(super) fn hide_window(window: &mut Window) {
    windows::hide_window(window);
}

#[cfg(target_os = "linux")]
pub(super) fn hide_window(window: &mut Window) {
    linux::hide_window(window);
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub(super) fn hide_window(window: &mut Window) {
    window.minimize_window();
}

#[cfg(target_os = "windows")]
pub(super) fn show_window(window: &mut Window) {
    windows::show_window(window);
}

#[cfg(target_os = "linux")]
pub(super) fn show_window(window: &mut Window) {
    linux::show_window(window);
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub(super) fn show_window(_window: &mut Window) {}

#[cfg(target_os = "windows")]
pub(super) fn notify_system(title: &str, message: &str, is_error: bool) {
    windows::notify_system(title, message, is_error);
}

#[cfg(target_os = "linux")]
pub(super) fn notify_system(title: &str, message: &str, is_error: bool) {
    linux::notify_system(title, message, is_error);
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub(super) fn notify_system(_title: &str, _message: &str, _is_error: bool) {}

#[cfg(target_os = "windows")]
pub(super) fn shutdown_tray() {
    windows::shutdown_tray();
}

#[cfg(target_os = "linux")]
pub(super) fn shutdown_tray() {
    linux::shutdown_tray();
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub(super) fn shutdown_tray() {}
