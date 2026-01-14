use gpui::*;
use gpui_component::Root;
use gpui_component::theme::{Theme, ThemeMode};
use r_wg::backend::wg::Engine;

use super::state::WgApp;

pub fn run() {
    let engine = Engine::new();

    Application::new().run(move |cx: &mut App| {
        gpui_component::init(cx);
        Theme::change(ThemeMode::Dark, None, cx);

        let engine = engine.clone();
        cx.open_window(WindowOptions::default(), move |window, cx| {
            let view = cx.new(|_cx| WgApp::new(engine));
            cx.new(|cx| Root::new(view, window, cx))
        })
        .unwrap();
    });
}
