use gpui::{SharedString, Window};
use gpui_component::notification::Notification;
use gpui_component::WindowExt;

use super::{SidebarItem, WgApp};

impl WgApp {
    pub(crate) fn set_sidebar_active(&mut self, value: SidebarItem, cx: &mut gpui::Context<Self>) {
        if self.ui_session.sidebar_active != value {
            self.ui_session.sidebar_active = value;
            if value != SidebarItem::Logs {
                self.stop_backend_log_polling();
            }
            cx.notify();
        }
    }

    pub(crate) fn set_sidebar_collapsed(&mut self, value: bool, cx: &mut gpui::Context<Self>) {
        if self.ui_session.sidebar_collapsed != value {
            self.ui_session.sidebar_collapsed = value;
            cx.notify();
        }
    }

    pub(crate) fn toggle_sidebar_collapsed(&mut self, cx: &mut gpui::Context<Self>) {
        let next = !self.ui_session.sidebar_collapsed;
        self.set_sidebar_collapsed(next, cx);
    }

    pub(crate) fn set_sidebar_overlay_open(&mut self, value: bool, cx: &mut gpui::Context<Self>) {
        if self.ui_session.sidebar_overlay_open != value {
            self.ui_session.sidebar_overlay_open = value;
            cx.notify();
        }
    }

    pub(crate) fn open_sidebar_overlay(&mut self, cx: &mut gpui::Context<Self>) {
        self.set_sidebar_overlay_open(true, cx);
    }

    pub(crate) fn close_sidebar_overlay(&mut self, cx: &mut gpui::Context<Self>) {
        self.set_sidebar_overlay_open(false, cx);
    }

    pub(crate) fn set_show_alternate_theme_preview(
        &mut self,
        value: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.ui_session.show_alternate_theme_preview != value {
            self.ui_session.show_alternate_theme_preview = value;
            cx.notify();
        }
    }

    pub(crate) fn push_success_toast(
        &mut self,
        message: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        window.push_notification(Notification::success(message.into()), cx);
    }
}
