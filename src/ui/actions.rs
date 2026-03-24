//! UI 操作入口：将原本集中在单文件中的逻辑拆分为多个子模块，
//! 通过职责分层降低耦合，便于维护与测试。

pub(crate) mod config;
mod import_export;
mod logs;
mod persistence;
mod route_map;
mod stats;
mod status;
mod theme;
mod tunnel;
