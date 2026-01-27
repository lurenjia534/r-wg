use std::collections::HashSet;
use std::env;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use chrono::Local;
use crossbeam_queue::ArrayQueue;
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::{layer::Context, layer::SubscriberExt, EnvFilter, Layer};

// UI 日志面板最多保留的行数（环形缓冲容量）。
const MAX_LOG_LINES: usize = 2000;
// tracing 事件的固定 target，便于统一过滤与输出。
const LOG_TARGET: &str = "r_wg";

pub mod events;

// 日志等级，数值越大越“详细”（更吵）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum LogLevel {
    Error = 1,
    Warn = 2,
    Info = 3,
    Debug = 4,
    Trace = 5,
}

impl LogLevel {
    // 解析环境变量值，支持英文名称与数字档位。
    fn parse(value: &str) -> Option<Self> {
        let normalized = value.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "error" | "err" | "1" => Some(LogLevel::Error),
            "warn" | "warning" | "2" => Some(LogLevel::Warn),
            "info" | "3" => Some(LogLevel::Info),
            "debug" | "4" => Some(LogLevel::Debug),
            "trace" | "5" => Some(LogLevel::Trace),
            _ => None,
        }
    }

    // 转为 tracing 的 EnvFilter 字符串表示。
    fn as_str(self) -> &'static str {
        match self {
            LogLevel::Error => "error",
            LogLevel::Warn => "warn",
            LogLevel::Info => "info",
            LogLevel::Debug => "debug",
            LogLevel::Trace => "trace",
        }
    }
}

// scope 用于功能域分组，便于按模块开关日志。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LogScope {
    Net,
    Engine,
    Stats,
    Dns,
    Ui,
    Other(&'static str),
}

impl LogScope {
    // scope 的字符串形式会进入日志文本与过滤逻辑。
    pub fn as_str(self) -> &'static str {
        match self {
            LogScope::Net => "net",
            LogScope::Engine => "engine",
            LogScope::Stats => "stats",
            LogScope::Dns => "dns",
            LogScope::Ui => "ui",
            LogScope::Other(value) => value,
        }
    }
}

// 运行时日志配置，来自环境变量。
#[derive(Debug, Clone)]
pub struct LogConfig {
    level: LogLevel,
    scopes: Option<HashSet<String>>,
    stderr_enabled: bool,
    buffer_enabled: bool,
}

impl LogConfig {
    // 从环境变量读取配置。
    // - RWG_LOG 为总开关（关闭时 stderr + buffer 都关闭）。
    // - RWG_LOG_LEVEL 控制等级。
    // - RWG_LOG_SCOPES 控制 scope 白名单。
    // - RWG_LOG_BUFFER 控制 UI 缓冲是否记录。
    fn from_env() -> Self {
        let log_enabled = match env::var("RWG_LOG") {
            Ok(value) => parse_bool(&value).unwrap_or(true),
            Err(_) => false,
        };
        let buffer_enabled = if log_enabled {
            match env::var("RWG_LOG_BUFFER") {
                Ok(value) => parse_bool(&value).unwrap_or(true),
                Err(_) => true,
            }
        } else {
            false
        };
        let level = env::var("RWG_LOG_LEVEL")
            .ok()
            .and_then(|value| LogLevel::parse(&value))
            .unwrap_or(LogLevel::Info);
        let scopes = env::var("RWG_LOG_SCOPES")
            .ok()
            .and_then(|value| parse_scopes(&value));

        LogConfig {
            level,
            scopes,
            stderr_enabled: log_enabled,
            buffer_enabled,
        }
    }

    // 是否有任意输出被启用（用于构建 filter）。
    fn any_enabled(&self) -> bool {
        self.stderr_enabled || self.buffer_enabled
    }

    // scope 是否允许输出（不设白名单则全部允许）。
    fn scope_allowed(&self, scope: &str) -> bool {
        if let Some(scopes) = &self.scopes {
            return scopes.contains(scope);
        }
        true
    }
}

// 使用无锁环形队列保存最近日志，供 UI 快速读取。
static LOG_BUFFER: OnceLock<ArrayQueue<String>> = OnceLock::new();
// 日志缓冲开关默认开启，与 stderr 输出开关独立。
static LOG_BUFFER_ENABLED: AtomicBool = AtomicBool::new(true);
// stderr 输出开关（来自 RWG_LOG）。
static LOG_STDERR_ENABLED: AtomicBool = AtomicBool::new(false);
// 全局配置只初始化一次。
static LOG_CONFIG: OnceLock<LogConfig> = OnceLock::new();

// 统一创建环形缓冲。
fn buffer() -> &'static ArrayQueue<String> {
    LOG_BUFFER.get_or_init(|| ArrayQueue::new(MAX_LOG_LINES))
}

// 初始化日志系统：注册全局 subscriber + 两个 sink（buffer 与 stderr）。
pub fn init() -> &'static LogConfig {
    LOG_CONFIG.get_or_init(|| {
        let config = LogConfig::from_env();
        LOG_BUFFER_ENABLED.store(config.buffer_enabled, Ordering::Relaxed);
        LOG_STDERR_ENABLED.store(config.stderr_enabled, Ordering::Relaxed);

        let filter = build_filter(&config);
        // 缓冲层始终注册，是否写入由 LOG_BUFFER_ENABLED 控制，
        // 这样运行时切换缓冲开关才有效。
        let buffer_layer = Some(BufferLayer::new());
        let stderr_layer = if config.stderr_enabled {
            Some(StderrLayer)
        } else {
            None
        };

        // EnvFilter 只对固定 target 生效，避免第三方库干扰。
        let subscriber = tracing_subscriber::registry()
            .with(filter)
            .with(buffer_layer)
            .with(stderr_layer);
        let _ = tracing::subscriber::set_global_default(subscriber);

        config
    })
}

// 确保日志系统已初始化。
fn ensure_init() {
    let _ = init();
}

// 获取配置（会触发初始化）。
pub fn config() -> &'static LogConfig {
    init()
}

// UI 缓冲是否开启。
pub fn buffer_enabled() -> bool {
    ensure_init();
    LOG_BUFFER_ENABLED.load(Ordering::Relaxed)
}

// 运行时切换 UI 缓冲开关（仅影响缓冲，不影响 stderr）。
pub fn set_buffer_enabled(enabled: bool) {
    ensure_init();
    let config = LOG_CONFIG.get().expect("log config initialized");
    if !config.stderr_enabled {
        // RWG_LOG 为总开关：关闭时不允许启用缓冲。
        LOG_BUFFER_ENABLED.store(false, Ordering::Relaxed);
        return;
    }
    LOG_BUFFER_ENABLED.store(enabled, Ordering::Relaxed);
}

// stderr 输出是否开启（由 RWG_LOG 控制）。
pub fn enabled() -> bool {
    // stderr 输出开关；缓冲是否记录由 buffer_enabled 控制。
    ensure_init();
    LOG_STDERR_ENABLED.load(Ordering::Relaxed)
}

// 统一判断某 scope + level 是否允许输出，避免昂贵格式化。
pub fn enabled_for(level: LogLevel, scope: &str) -> bool {
    ensure_init();
    if !LOG_BUFFER_ENABLED.load(Ordering::Relaxed) && !LOG_STDERR_ENABLED.load(Ordering::Relaxed) {
        return false;
    }
    let config = LOG_CONFIG.get().expect("log config initialized");
    if level > config.level {
        return false;
    }
    config.scope_allowed(scope)
}

// 兼容旧的 log::log API（默认 INFO）。
pub fn log(scope: &str, message: String) {
    event(LogLevel::Info, scope, format_args!("{message}"));
}

// 统一事件入口：把 scope 作为字段写入 tracing。
pub fn event(level: LogLevel, scope: &str, args: fmt::Arguments) {
    ensure_init();
    if !enabled_for(level, scope) {
        return;
    }
    match level {
        LogLevel::Error => {
            tracing::event!(target: LOG_TARGET, Level::ERROR, scope = scope, message = %args)
        }
        LogLevel::Warn => {
            tracing::event!(target: LOG_TARGET, Level::WARN, scope = scope, message = %args)
        }
        LogLevel::Info => {
            tracing::event!(target: LOG_TARGET, Level::INFO, scope = scope, message = %args)
        }
        LogLevel::Debug => {
            tracing::event!(target: LOG_TARGET, Level::DEBUG, scope = scope, message = %args)
        }
        LogLevel::Trace => {
            tracing::event!(target: LOG_TARGET, Level::TRACE, scope = scope, message = %args)
        }
    }
}

#[macro_export]
macro_rules! log_error {
    ($scope:expr, $fmt:literal $(, $arg:expr)* $(,)?) => {{
        if $crate::log::enabled_for($crate::log::LogLevel::Error, $scope) {
            $crate::log::event(
                $crate::log::LogLevel::Error,
                $scope,
                format_args!($fmt, $($arg),*),
            );
        }
    }};
    ($scope:expr, $message:expr $(,)?) => {{
        if $crate::log::enabled_for($crate::log::LogLevel::Error, $scope) {
            $crate::log::event(
                $crate::log::LogLevel::Error,
                $scope,
                format_args!("{}", $message),
            );
        }
    }};
}

#[macro_export]
macro_rules! log_warn {
    ($scope:expr, $fmt:literal $(, $arg:expr)* $(,)?) => {{
        if $crate::log::enabled_for($crate::log::LogLevel::Warn, $scope) {
            $crate::log::event(
                $crate::log::LogLevel::Warn,
                $scope,
                format_args!($fmt, $($arg),*),
            );
        }
    }};
    ($scope:expr, $message:expr $(,)?) => {{
        if $crate::log::enabled_for($crate::log::LogLevel::Warn, $scope) {
            $crate::log::event(
                $crate::log::LogLevel::Warn,
                $scope,
                format_args!("{}", $message),
            );
        }
    }};
}

#[macro_export]
macro_rules! log_info {
    ($scope:expr, $fmt:literal $(, $arg:expr)* $(,)?) => {{
        if $crate::log::enabled_for($crate::log::LogLevel::Info, $scope) {
            $crate::log::event(
                $crate::log::LogLevel::Info,
                $scope,
                format_args!($fmt, $($arg),*),
            );
        }
    }};
    ($scope:expr, $message:expr $(,)?) => {{
        if $crate::log::enabled_for($crate::log::LogLevel::Info, $scope) {
            $crate::log::event(
                $crate::log::LogLevel::Info,
                $scope,
                format_args!("{}", $message),
            );
        }
    }};
}

#[macro_export]
macro_rules! log_debug {
    ($scope:expr, $fmt:literal $(, $arg:expr)* $(,)?) => {{
        if $crate::log::enabled_for($crate::log::LogLevel::Debug, $scope) {
            $crate::log::event(
                $crate::log::LogLevel::Debug,
                $scope,
                format_args!($fmt, $($arg),*),
            );
        }
    }};
    ($scope:expr, $message:expr $(,)?) => {{
        if $crate::log::enabled_for($crate::log::LogLevel::Debug, $scope) {
            $crate::log::event(
                $crate::log::LogLevel::Debug,
                $scope,
                format_args!("{}", $message),
            );
        }
    }};
}

#[macro_export]
macro_rules! log_trace {
    ($scope:expr, $fmt:literal $(, $arg:expr)* $(,)?) => {{
        if $crate::log::enabled_for($crate::log::LogLevel::Trace, $scope) {
            $crate::log::event(
                $crate::log::LogLevel::Trace,
                $scope,
                format_args!($fmt, $($arg),*),
            );
        }
    }};
    ($scope:expr, $message:expr $(,)?) => {{
        if $crate::log::enabled_for($crate::log::LogLevel::Trace, $scope) {
            $crate::log::event(
                $crate::log::LogLevel::Trace,
                $scope,
                format_args!("{}", $message),
            );
        }
    }};
}

// 获取当前缓冲快照（用于 UI 展示）。
pub fn snapshot() -> Vec<String> {
    ensure_init();
    if !LOG_BUFFER_ENABLED.load(Ordering::Relaxed) {
        return Vec::new();
    }
    let Some(buffer) = LOG_BUFFER.get() else {
        return Vec::new();
    };
    let mut lines = Vec::with_capacity(MAX_LOG_LINES);
    // 无锁快照：有限次出队再回填；如并发写入导致队列变满则提前停止。
    let count = buffer.len().min(MAX_LOG_LINES);
    for _ in 0..count {
        match buffer.pop() {
            Some(line) => lines.push(line),
            None => break,
        }
    }
    if !lines.is_empty() {
        for line in &lines {
            if buffer.push(line.clone()).is_err() {
                break;
            }
        }
    }
    lines
}

// 清空缓冲（用于 UI 清除按钮）。
pub fn clear() {
    ensure_init();
    if !LOG_BUFFER_ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let Some(buffer) = LOG_BUFFER.get() else {
        return;
    };
    for _ in 0..MAX_LOG_LINES {
        if buffer.pop().is_none() {
            break;
        }
    }
}

fn format_timestamp() -> String {
    // 本地时间戳，便于人工阅读。
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

// 仅过滤当前应用的 target，避免第三方库日志干扰。
fn build_filter(config: &LogConfig) -> EnvFilter {
    if !config.any_enabled() {
        return EnvFilter::new("off");
    }
    let level = config.level.as_str();
    EnvFilter::new(format!("{LOG_TARGET}={level}"))
}

// 解析通用布尔字符串。
fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

// 解析 scope 白名单；为空或包含 * / all 则视为全部允许。
fn parse_scopes(value: &str) -> Option<HashSet<String>> {
    let scopes: HashSet<String> = value
        .split(',')
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .map(|item| item.to_string())
        .collect();
    if scopes.is_empty() {
        return None;
    }
    if scopes
        .iter()
        .any(|item| item == "*" || item.eq_ignore_ascii_case("all"))
    {
        return None;
    }
    Some(scopes)
}

// 解析 tracing 事件字段，抽出 message 与 scope。
#[derive(Default)]
struct FieldVisitor {
    message: Option<String>,
    scope: Option<String>,
    fields: Vec<String>,
}

impl tracing::field::Visit for FieldVisitor {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        } else if field.name() == "scope" {
            self.scope = Some(value.to_string());
        } else {
            self.fields.push(format!("{}={value}", field.name()));
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{value:?}"));
        } else if field.name() == "scope" {
            self.scope = Some(format!("{value:?}"));
        } else {
            self.fields.push(format!("{}={value:?}", field.name()));
        }
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.fields.push(format!("{}={value}", field.name()));
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.fields.push(format!("{}={value}", field.name()));
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.fields.push(format!("{}={value}", field.name()));
    }

    fn record_f64(&mut self, field: &tracing::field::Field, value: f64) {
        self.fields.push(format!("{}={value}", field.name()));
    }
}

// 将 tracing 事件格式化为统一文本行。
fn format_event_line(event: &Event<'_>) -> String {
    let timestamp = format_timestamp();
    let mut visitor = FieldVisitor::default();
    event.record(&mut visitor);
    let scope = visitor
        .scope
        .as_deref()
        .unwrap_or_else(|| event.metadata().target());
    let message = if let Some(message) = visitor.message {
        message
    } else if !visitor.fields.is_empty() {
        visitor.fields.join(" ")
    } else {
        "event".to_string()
    };
    format!("[{timestamp}][r-wg][{scope}] {message}")
}

// 缓冲层：把事件写入环形队列供 UI 读取。
struct BufferLayer {
    buffer: &'static ArrayQueue<String>,
}

impl BufferLayer {
    fn new() -> Self {
        Self { buffer: buffer() }
    }
}

impl<S> Layer<S> for BufferLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        if !LOG_BUFFER_ENABLED.load(Ordering::Relaxed) {
            return;
        }
        if event.metadata().target() != LOG_TARGET {
            return;
        }
        let line = format_event_line(event);
        let _ = self.buffer.force_push(line);
    }
}

// stderr 输出层：用于开发调试。
struct StderrLayer;

impl<S> Layer<S> for StderrLayer
where
    S: Subscriber,
{
    #[allow(clippy::print_stderr)]
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        if !LOG_STDERR_ENABLED.load(Ordering::Relaxed) {
            return;
        }
        if event.metadata().target() != LOG_TARGET {
            return;
        }
        let line = format_event_line(event);
        eprintln!("{line}");
    }
}
