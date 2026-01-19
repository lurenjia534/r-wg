use gpui::*;
use gpui_component::Root;
use gpui_component_assets::Assets;
use gpui_component::theme::{Theme, ThemeMode};
use r_wg::backend::wg::Engine;

use super::persistence;
use super::state::WgApp;

pub fn run() {
    let engine = Engine::new();

    Application::new().with_assets(Assets).run(move |cx: &mut App| {
        gpui_component::init(cx);
        // 打开窗口前加载主题选择。
        let theme_mode = load_persisted_theme_mode();
        Theme::change(theme_mode, None, cx);

        let engine = engine.clone();
        cx.open_window(WindowOptions::default(), move |window, cx| {
            let view = cx.new(|_cx| WgApp::new(engine, theme_mode));
            cx.new(|cx| Root::new(view, window, cx))
        })
        .unwrap();
    });
}

fn load_persisted_theme_mode() -> ThemeMode {
    // 失败则回退深色模式。
    let storage = match persistence::ensure_storage_dirs() {
        Ok(storage) => storage,
        Err(_) => return ThemeMode::Dark,
    };
    persistence::load_state(&storage)
        .ok()
        .flatten()
        .and_then(|state| state.theme_mode)
        .unwrap_or(ThemeMode::Dark)
}
