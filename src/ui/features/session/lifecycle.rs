use gpui::{App, AppContext, AsyncApp, WeakEntity};
use r_wg::backend::wg::Engine;
#[cfg(not(target_os = "windows"))]
use r_wg::backend::wg::EngineError;
#[cfg(target_os = "windows")]
use r_wg::backend::wg::EngineStatus;

use crate::ui::state::WgApp;

use super::controller;

#[cfg(target_os = "windows")]
pub(crate) fn sync_engine_status(view: WeakEntity<WgApp>, engine: Engine, cx: &mut App) {
    cx.spawn(async move |cx| {
        let (status_result, apply_report) = cx
            .background_spawn(async move {
                let status = engine.status();
                let apply_report = engine.apply_report().ok().flatten();
                (status, apply_report)
            })
            .await;
        let _ = view.update(cx, |this, cx| {
            if !matches!(status_result, Ok(EngineStatus::Running)) {
                return;
            }
            this.runtime.running = true;
            this.runtime.busy = false;
            this.runtime.set_last_apply_report(apply_report);
            // helper 恢复场景下不一定拿得到原始配置名，先放通用占位避免 UI 空白。
            if this.runtime.running_name.is_none() {
                this.runtime.running_name = Some("Tunnel".to_string());
            }
            // 这里只恢复运行态与统计轮询，不推断具体配置来源。
            this.set_status("Tunnel running");
            this.stats.reset_for_start();
            this.start_stats_polling(cx);
            cx.notify();
        });
    })
    .detach();
}

pub(crate) fn sync_apply_report(view: WeakEntity<WgApp>, engine: Engine, cx: &mut App) {
    cx.spawn(async move |cx| {
        let result = cx
            .background_spawn(async move { engine.apply_report() })
            .await;
        let _ = view.update(cx, |this, cx| {
            if let Ok(report) = result {
                this.runtime.set_last_apply_report(report);
                cx.notify();
            }
        });
    })
    .detach();
}

#[cfg(not(target_os = "windows"))]
pub(crate) async fn request_shutdown_stop(
    view: WeakEntity<WgApp>,
    engine: Engine,
    cx: &mut AsyncApp,
) -> bool {
    let mut was_running = false;
    let _ = view.update(cx, |this, cx| {
        was_running = this.runtime.running;
        if this.runtime.running {
            this.runtime.busy = true;
            this.set_status("Stopping...");
            cx.notify();
        }
    });

    let result = cx
        .background_executor()
        .spawn(async move { engine.stop() })
        .await;
    let should_finish = matches!(
        &result,
        Ok(()) | Err(EngineError::NotRunning) | Err(EngineError::ChannelClosed)
    );

    let _ = view.update(cx, |this, cx| {
        if should_finish {
            if was_running {
                controller::complete_stop_success(this, cx);
            }
        } else if let Err(err) = result {
            if was_running {
                this.runtime.busy = false;
            }
            controller::complete_stop_failure(this, format!("Stop failed: {err}"));
        }
        cx.notify();
    });

    should_finish
}
