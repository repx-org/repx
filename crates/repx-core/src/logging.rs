use crate::config::LoggingConfig;
use crate::errors::ConfigError;
use chrono::Local;
use std::env;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};
use tracing::Level;
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::time::FormatTime;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

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

impl From<LogLevel> for Level {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Error => Level::ERROR,
            LogLevel::Warn => Level::WARN,
            LogLevel::Info => Level::INFO,
            LogLevel::Debug => Level::DEBUG,
            LogLevel::Trace => Level::TRACE,
        }
    }
}

static DEFAULT_LOG_LEVEL: Mutex<LogLevel> = Mutex::new(LogLevel::Info);

pub fn set_log_level(level: LogLevel) {
    if let Ok(mut default_level) = DEFAULT_LOG_LEVEL.lock() {
        *default_level = level;
    }
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

fn get_default_log_level() -> Level {
    DEFAULT_LOG_LEVEL
        .lock()
        .map(|level| (*level).into())
        .unwrap_or(Level::INFO)
}

struct LocalTimeFormatter;

impl FormatTime for LocalTimeFormatter {
    fn format_time(&self, w: &mut Writer<'_>) -> std::fmt::Result {
        let now = Local::now();
        write!(w, "{}", now.format("%Y-%m-%d %H:%M:%S"))
    }
}

fn rotate_logs(log_dir: &Path, prefix: &str, config: &LoggingConfig) -> Result<(), ConfigError> {
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

fn init_tracing_subscriber(log_path: &Path) -> Result<(), ConfigError> {
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;

    let workspace_root = env::var("CARGO_MANIFEST_DIR")
        .ok()
        .and_then(|p| {
            let path = PathBuf::from(p);
            path.parent()?.parent().map(|p| p.to_path_buf())
        })
        .or_else(|| {
            let mut current = env::current_dir().ok()?;
            loop {
                if current.join("Cargo.toml").exists() {
                    if let Ok(content) = std::fs::read_to_string(current.join("Cargo.toml")) {
                        if content.contains("[workspace]") {
                            return Some(current);
                        }
                    }
                }
                current = current.parent()?.to_path_buf();
            }
        })
        .unwrap_or_else(|| env::current_dir().unwrap_or_default());

    let default_level = get_default_log_level();
    let level_str = match default_level {
        Level::ERROR => "error",
        Level::WARN => "warn",
        Level::INFO => "info",
        Level::DEBUG => "debug",
        Level::TRACE => "trace",
    };

    let env_filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(level_str))
        .unwrap_or_else(|_| EnvFilter::new("info"));

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(Mutex::new(log_file))
        .with_timer(LocalTimeFormatter)
        .with_ansi(false)
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(false)
        .with_line_number(true)
        .with_file(true)
        .with_level(true)
        .event_format(CustomFormatter {
            workspace_root: workspace_root.clone(),
        });

    if env::var("REPX_TEST_LOG_TEE").is_ok() {
        let stderr_layer = tracing_subscriber::fmt::layer()
            .with_writer(std::io::stderr)
            .with_timer(LocalTimeFormatter)
            .with_ansi(false)
            .with_target(false)
            .with_line_number(true)
            .with_file(true)
            .with_level(true)
            .event_format(CustomFormatter { workspace_root });

        tracing_subscriber::registry()
            .with(env_filter)
            .with(file_layer)
            .with(stderr_layer)
            .init();
    } else {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(file_layer)
            .init();
    }

    tracing::info!("--- Logger Initialized ---");

    Ok(())
}

struct CustomFormatter {
    workspace_root: PathBuf,
}

impl<S, N> tracing_subscriber::fmt::FormatEvent<S, N> for CustomFormatter
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
    N: for<'a> tracing_subscriber::fmt::FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &tracing_subscriber::fmt::FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> std::fmt::Result {
        let metadata = event.metadata();

        write!(writer, "[")?;
        LocalTimeFormatter.format_time(&mut writer)?;
        write!(writer, "] ")?;

        let level = metadata.level();
        write!(writer, "[{:5}] ", level)?;

        if let Some(file) = metadata.file() {
            let display_path = if file.starts_with('/') {
                let file_path = PathBuf::from(file);
                if let Ok(rel) = file_path.strip_prefix(&self.workspace_root) {
                    rel.to_string_lossy().to_string()
                } else {
                    file.to_string()
                }
            } else if let Some(module) = metadata.module_path() {
                let parts: Vec<&str> = module.split("::").collect();
                if let Some(crate_name) = parts.first() {
                    let crate_dir = crate_name.replace('_', "-");
                    format!("crates/{}/{}", crate_dir, file)
                } else {
                    file.to_string()
                }
            } else {
                file.to_string()
            };

            write!(writer, "{}:{} ", display_path, metadata.line().unwrap_or(0))?;
        }

        ctx.field_format().format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

pub fn init_session_logger(config: &LoggingConfig) -> Result<(), ConfigError> {
    let xdg_dirs = xdg::BaseDirectories::with_prefix("repx");
    let cache_home = xdg_dirs.get_cache_home().ok_or_else(|| {
        ConfigError::Io(std::io::Error::new(
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

    init_tracing_subscriber(&log_path)?;

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

pub fn init_stderr_logger() {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(get_default_log_level().to_string()));

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(std::io::stderr)
        .with_timer(LocalTimeFormatter)
        .with_ansi(true)
        .with_target(false)
        .with_line_number(false)
        .with_file(false)
        .with_level(true)
        .init();
}

pub fn init_tui_logger(config: &LoggingConfig) -> Result<(), ConfigError> {
    let xdg_dirs = xdg::BaseDirectories::with_prefix("repx");
    let cache_home = xdg_dirs.get_cache_home().ok_or_else(|| {
        ConfigError::Io(std::io::Error::new(
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

    init_tracing_subscriber(&log_path)?;

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
    tracing::debug!("[CMD] {}", command_str);
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
