use crate::config::LoggingConfig;
use crate::error::AppError;
use chrono::Local;
use once_cell::sync::Lazy;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

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

fn rotate_logs(log_dir: &Path, prefix: &str, config: &LoggingConfig) -> Result<(), AppError> {
    if !log_dir.exists() {
        fs::create_dir_all(log_dir)?;
    }

    let mut entries: Vec<PathBuf> = fs::read_dir(log_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(prefix) && n.ends_with(".log"))
        })
        .collect();

    entries.sort();

    if config.max_files > 0 && entries.len() > config.max_files {
        let to_delete = entries.len() - config.max_files;
        for path in entries.drain(0..to_delete) {
            let _ = fs::remove_file(path);
        }
    }

    if config.max_age_days > 0 {
        let now = SystemTime::now();
        let max_age = Duration::from_secs(config.max_age_days * 24 * 60 * 60);

        entries.retain(|path| {
            let name = path.file_name().unwrap().to_string_lossy();
            let parts: Vec<&str> = name.split('_').collect();
            if parts.len() >= 2 {
                if let Ok(date) = chrono::NaiveDate::parse_from_str(parts[1], "%Y-%m-%d") {
                    let log_time = date
                        .and_hms_opt(0, 0, 0)
                        .unwrap()
                        .and_local_timezone(chrono::Local)
                        .unwrap();
                    let log_sys_time = SystemTime::from(log_time);
                    if let Ok(age) = now.duration_since(log_sys_time) {
                        if age > max_age {
                            let _ = fs::remove_file(path);
                            return false;
                        }
                    }
                }
            }
            true
        });
    }

    Ok(())
}

pub fn init_session_logger(config: &LoggingConfig) -> Result<(), AppError> {
    let xdg_dirs = xdg::BaseDirectories::with_prefix("repx");
    let cache_home = xdg_dirs.get_cache_home().ok_or_else(|| {
        AppError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Could not find cache home directory",
        ))
    })?;
    let logs_dir = cache_home.join("logs");

    rotate_logs(&logs_dir, "repx_", config)?;

    let timestamp = Local::now().format("%Y-%m-%d_%H-%M-%S");
    let pid = std::process::id();
    let filename = format!("repx_{}_{}.log", timestamp, pid);
    let log_path = logs_dir.join(&filename);

    init_logger(&log_path)?;

    let symlink_path = cache_home.join("repx.log");
    let _ = fs::remove_file(&symlink_path);
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let target = Path::new("logs").join(filename);
        let _ = symlink(&target, &symlink_path);
    }

    Ok(())
}

pub fn init_tui_logger(config: &LoggingConfig) -> Result<(), AppError> {
    let xdg_dirs = xdg::BaseDirectories::with_prefix("repx");
    let cache_home = xdg_dirs.get_cache_home().ok_or_else(|| {
        AppError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Could not find cache home directory",
        ))
    })?;
    let logs_dir = cache_home.join("logs");

    rotate_logs(&logs_dir, "repx-tui_", config)?;

    let timestamp = Local::now().format("%Y-%m-%d_%H-%M-%S");
    let pid = std::process::id();
    let filename = format!("repx-tui_{}_{}.log", timestamp, pid);
    let log_path = logs_dir.join(&filename);

    init_logger(&log_path)?;

    let symlink_path = cache_home.join("repx-tui.log");
    let _ = fs::remove_file(&symlink_path);
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let target = Path::new("logs").join(filename);
        let _ = symlink(&target, &symlink_path);
    }

    Ok(())
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
        } else {
            eprint!("{}", log_line);
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

#[cfg(test)]
mod tests {
    use super::rotate_logs;
    use crate::config::LoggingConfig;
    use chrono::{Duration as ChronoDuration, Local};
    use std::fs::File;
    use tempfile::tempdir;

    #[test]
    fn test_rotate_logs_max_files() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        let filenames = vec![
            "repx_2023-01-01_10-00-00_1.log",
            "repx_2023-01-02_10-00-00_1.log",
            "repx_2023-01-03_10-00-00_1.log",
            "repx_2023-01-04_10-00-00_1.log",
            "repx_2023-01-05_10-00-00_1.log",
        ];

        for name in &filenames {
            File::create(path.join(name)).unwrap();
        }

        File::create(path.join("other.txt")).unwrap();

        let config = LoggingConfig {
            max_files: 3,
            max_age_days: 0,
        };

        rotate_logs(path, "repx_", &config).unwrap();

        assert!(
            !path.join(filenames[0]).exists(),
            "Oldest file should be deleted"
        );
        assert!(
            !path.join(filenames[1]).exists(),
            "Second oldest file should be deleted"
        );
        assert!(path.join(filenames[2]).exists(), "File 3 should exist");
        assert!(path.join(filenames[3]).exists(), "File 4 should exist");
        assert!(path.join(filenames[4]).exists(), "Newest file should exist");
        assert!(
            path.join("other.txt").exists(),
            "Non-log file should be preserved"
        );
    }

    #[test]
    fn test_rotate_logs_max_age() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        let now = Local::now();
        let yesterday = now - ChronoDuration::days(1);
        let eight_days_ago = now - ChronoDuration::days(8);

        let fmt = "%Y-%m-%d";

        let name_now = format!("repx_{}_10-00-00_1.log", now.format(fmt));
        let name_yesterday = format!("repx_{}_10-00-00_1.log", yesterday.format(fmt));
        let name_old = format!("repx_{}_10-00-00_1.log", eight_days_ago.format(fmt));

        File::create(path.join(&name_now)).unwrap();
        File::create(path.join(&name_yesterday)).unwrap();
        File::create(path.join(&name_old)).unwrap();

        let config = LoggingConfig {
            max_files: 0,
            max_age_days: 7,
        };

        rotate_logs(path, "repx_", &config).unwrap();

        assert!(path.join(&name_now).exists(), "Current file should exist");
        assert!(
            path.join(&name_yesterday).exists(),
            "Yesterday's file should exist"
        );
        assert!(!path.join(&name_old).exists(), "Old file should be deleted");
    }
}
