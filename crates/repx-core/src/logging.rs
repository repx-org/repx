use crate::error::AppError;
use chrono::Local;
use once_cell::sync::Lazy;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Error = 0,
    Warn = 1,
    Info = 2,
    Debug = 3,
    Trace = 4,
}

impl From<u8> for LogLevel {
    fn from(val: u8) -> Self {
        match val {
            0 => LogLevel::Error,
            1 => LogLevel::Warn,
            2 => LogLevel::Info,
            3 => LogLevel::Debug,
            _ => LogLevel::Trace,
        }
    }
}

pub static MAX_LOG_LEVEL: AtomicUsize = AtomicUsize::new(LogLevel::Info as usize);

pub fn set_log_level(level: LogLevel) {
    MAX_LOG_LEVEL.store(level as usize, Ordering::Relaxed);
}

pub fn set_log_level_from_env() {
    if let Ok(level) = env::var("REPX_LOG_LEVEL") {
        match level.to_uppercase().as_str() {
            "TRACE" => set_log_level(LogLevel::Trace),
            "DEBUG" => set_log_level(LogLevel::Debug),
            "INFO" => set_log_level(LogLevel::Info),
            "WARN" => set_log_level(LogLevel::Warn),
            "ERROR" => set_log_level(LogLevel::Error),
            _ => {}
        }
    }
}

static LOG_FILE: Lazy<Mutex<Option<File>>> = Lazy::new(|| Mutex::new(None));

pub fn init_logger(log_path: &Path) -> Result<(), AppError> {
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;

    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
    let log_line = format!("[{}] [INFO] --- Logger Initialized ---\n", timestamp);
    let _ = file.write_all(log_line.as_bytes());

    let mut log_file_guard = LOG_FILE.lock().unwrap();
    *log_file_guard = Some(file);

    Ok(())
}

pub fn init_cli_logger() -> Result<(), AppError> {
    let xdg_dirs = xdg::BaseDirectories::with_prefix("repx");
    let log_path = xdg_dirs.place_cache_file("repx.log")?;
    init_logger(&log_path)
}

pub fn init_tui_logger() -> Result<(), AppError> {
    let xdg_dirs = xdg::BaseDirectories::with_prefix("repx");
    let log_path = xdg_dirs.place_cache_file("repx-tui.log")?;
    init_logger(&log_path)
}

#[doc(hidden)]
pub fn __write_log_entry(level: &str, message: &str) {
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
    let log_line = format!("[{}] [{}] {}\n", timestamp, level, message);

    if env::var("REPX_TEST_LOG_TEE").is_ok() {
        eprintln!("[LOG-TEE] {}", log_line.trim());
    }

    if let Ok(mut log_file_guard) = LOG_FILE.lock() {
        if let Some(file) = log_file_guard.as_mut() {
            let _ = file.write_all(log_line.as_bytes());
        }
    }
}

#[macro_export]
macro_rules! log_message {
    ($level:expr, $level_str:expr, $($arg:tt)+) => {
        if $crate::logging::MAX_LOG_LEVEL.load(std::sync::atomic::Ordering::Relaxed) >= $level as usize {
            let msg = format!($($arg)+);
            $crate::logging::__write_log_entry($level_str, &msg);
        }
    };
}

#[macro_export]
macro_rules! log_trace {
    ($($arg:tt)+) => ($crate::log_message!($crate::logging::LogLevel::Trace, "TRACE", $($arg)+));
}

#[macro_export]
macro_rules! log_debug {
    ($($arg:tt)+) => ($crate::log_message!($crate::logging::LogLevel::Debug, "DEBUG", $($arg)+));
}

#[macro_export]
macro_rules! log_info {
    ($($arg:tt)+) => ($crate::log_message!($crate::logging::LogLevel::Info, "INFO", $($arg)+));
}

#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)+) => ($crate::log_message!($crate::logging::LogLevel::Warn, "WARN", $($arg)+));
}

#[macro_export]
macro_rules! log_error {
    ($($arg:tt)+) => ($crate::log_message!($crate::logging::LogLevel::Error, "ERROR", $($arg)+));
}

fn format_command_for_display(command: &Command) -> String {
    let program = command.get_program().to_string_lossy();
    let args = command
        .get_args()
        .map(|arg| {
            let s = arg.to_string_lossy();
            if s.contains(char::is_whitespace) || s.is_empty() {
                format!("'{}'", s)
            } else {
                s.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    format!("{} {}", program, args)
}

pub fn log_and_print_command(command: &Command) {
    let command_str = format_command_for_display(command);
    log_debug!("[CMD] {}", command_str);
}
