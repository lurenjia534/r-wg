mod actions;
mod app;
mod commands;
mod features;
mod format;
mod i18n;
mod permissions;
mod persistence;
pub(crate) mod single_instance;
mod state;
mod tray;
mod view;

pub use app::run;
