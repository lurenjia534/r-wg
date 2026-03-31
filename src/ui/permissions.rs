use gpui::SharedString;
use r_wg::application::DiagnosticsService;

pub(crate) fn start_permission_message() -> Option<SharedString> {
    DiagnosticsService::new()
        .start_permission_message()
        .map(Into::into)
}
