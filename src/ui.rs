mod actions;
mod app;
mod format;
mod permissions;
mod persistence;
pub(crate) mod single_instance;
mod state;
mod theme_lint;
mod themes;
mod tray;
mod view;

pub use app::run;
