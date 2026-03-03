use crate::errors::ConfigError;
use crate::theme;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use xdg::BaseDirectories;

const CONFIG_FILE_NAME: &str = "config.toml";
const THEME_FILE_NAME: &str = "theme.toml";
const RESOURCES_FILE_NAME: &str = "resources.toml";

const DEFAULT_CONFIG_CONTENT: &str = include_str!("defaults/config.toml");
const DEFAULT_RESOURCES_CONTENT: &str = include_str!("defaults/resources.toml");

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct SchedulerConfig {
    #[serde(default)]
    pub execution_types: Vec<crate::model::ExecutionType>,
    pub local_concurrency: Option<usize>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub address: Option<String>,
    pub base_path: PathBuf,
    pub node_local_path: Option<PathBuf>,
    pub default_scheduler: Option<crate::model::SchedulerType>,
    pub default_execution_type: Option<crate::model::ExecutionType>,
    #[serde(default)]
    pub mount_host_paths: bool,
    #[serde(default)]
    pub mount_paths: Vec<String>,
    #[serde(default)]
    pub local: Option<SchedulerConfig>,
    #[serde(default)]
    pub slurm: Option<SchedulerConfig>,
}

const TUI_DEFAULT_TICK_RATE_MS: u64 = 1000;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct LoggingConfig {
    #[serde(default = "default_max_files")]
    pub max_files: usize,
    #[serde(default = "default_max_age_days")]
    pub max_age_days: u64,
}

fn default_max_files() -> usize {
    50
}

fn default_max_age_days() -> u64 {
    7
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            max_files: default_max_files(),
            max_age_days: default_max_age_days(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub theme: Option<String>,
    pub submission_target: Option<String>,
    pub default_scheduler: Option<crate::model::SchedulerType>,
    #[serde(default)]
    pub logging: LoggingConfig,
    #[serde(default)]
    pub targets: BTreeMap<String, Target>,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct ResourceRule {
    pub job_id_glob: Option<String>,
    pub target: Option<String>,
    pub partition: Option<String>,
    #[serde(rename = "cpus-per-task")]
    pub cpus_per_task: Option<u32>,
    pub mem: Option<String>,
    pub time: Option<String>,
    #[serde(default)]
    pub sbatch_opts: Vec<String>,
    #[serde(default)]
    pub worker_resources: Option<Box<ResourceRule>>,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct Resources {
    #[serde(default)]
    pub defaults: ResourceRule,
    #[serde(default)]
    pub rules: Vec<ResourceRule>,
}

impl Config {
    pub fn tui_tick_rate(&self) -> Duration {
        Duration::from_millis(TUI_DEFAULT_TICK_RATE_MS)
    }
}

fn create_default_config_if_missing(xdg_dirs: &BaseDirectories) -> Result<PathBuf, ConfigError> {
    match xdg_dirs.find_config_file(CONFIG_FILE_NAME) {
        Some(path) => Ok(path),
        None => {
            let config_path = xdg_dirs.place_config_file(CONFIG_FILE_NAME)?;
            fs::write(&config_path, DEFAULT_CONFIG_CONTENT)?;
            Ok(config_path)
        }
    }
}

fn create_default_theme_if_missing(xdg_dirs: &BaseDirectories) -> Result<(), ConfigError> {
    if xdg_dirs.find_config_file(THEME_FILE_NAME).is_none() {
        let theme_path = xdg_dirs.place_config_file(THEME_FILE_NAME)?;
        let default_theme = theme::default_theme();
        let theme_toml = toml::to_string_pretty(&default_theme).map_err(std::io::Error::other)?;
        fs::write(theme_path, theme_toml)?;
    }
    Ok(())
}

fn create_default_resources_if_missing(xdg_dirs: &BaseDirectories) -> Result<(), ConfigError> {
    if xdg_dirs.find_config_file(RESOURCES_FILE_NAME).is_none() {
        let resources_path = xdg_dirs.place_config_file(RESOURCES_FILE_NAME)?;
        fs::write(resources_path, DEFAULT_RESOURCES_CONTENT)?;
    }
    Ok(())
}
pub fn merge_toml_values(a: &mut toml::Value, b: &toml::Value) {
    match (a, b) {
        (toml::Value::Table(a), toml::Value::Table(b)) => {
            for (k, v) in b {
                merge_toml_values(a.entry(k.clone()).or_insert(v.clone()), v);
            }
        }
        (a, b) => {
            *a = b.clone();
        }
    }
}

pub fn load_resources(
    extra_path: Option<&std::path::Path>,
) -> Result<Option<Resources>, ConfigError> {
    let mut merged_value = toml::Value::Table(toml::map::Map::new());

    let xdg_dirs = crate::xdg_dirs();
    if let Some(global_path) = xdg_dirs.find_config_file(RESOURCES_FILE_NAME) {
        tracing::debug!("Loading global resources from: {}", global_path.display());
        let content = fs::read_to_string(global_path)?;
        let global_value: toml::Value = toml::from_str(&content).map_err(ConfigError::Toml)?;
        merge_toml_values(&mut merged_value, &global_value);
    }

    let cwd_path = std::env::current_dir()?.join(RESOURCES_FILE_NAME);
    if cwd_path.exists() {
        tracing::debug!("Loading local resources from: {}", cwd_path.display());
        let content = fs::read_to_string(cwd_path)?;
        let local_value: toml::Value = toml::from_str(&content).map_err(ConfigError::Toml)?;
        merge_toml_values(&mut merged_value, &local_value);
    }

    if let Some(path) = extra_path {
        if path.exists() {
            tracing::debug!("Loading specified resources from: {}", path.display());
            let content = fs::read_to_string(path)?;
            let cli_value: toml::Value = toml::from_str(&content).map_err(ConfigError::Toml)?;
            merge_toml_values(&mut merged_value, &cli_value);
        } else {
            return Err(ConfigError::PathIo {
                path: path.to_path_buf(),
                source: std::io::Error::new(std::io::ErrorKind::NotFound, "File not found"),
            });
        }
    }

    if merged_value.as_table().is_none_or(|t| t.is_empty()) {
        Ok(None)
    } else {
        let final_resources: Resources = merged_value.try_into().map_err(ConfigError::Toml)?;
        Ok(Some(final_resources))
    }
}

pub fn load_config() -> Result<Config, ConfigError> {
    let xdg_dirs = crate::xdg_dirs();

    let config_path = create_default_config_if_missing(xdg_dirs)?;
    create_default_theme_if_missing(xdg_dirs)?;
    create_default_resources_if_missing(xdg_dirs)?;

    let file_content = fs::read_to_string(config_path)?;
    let mut config: Config = toml::from_str(&file_content)?;

    for (name, target) in config.targets.iter_mut() {
        let path_str = target.base_path.display().to_string();
        let expanded_path_str = shellexpand::tilde(&path_str).into_owned();
        target.base_path = PathBuf::from(&expanded_path_str);

        if let Some(local_path) = &target.node_local_path {
            let local_str = local_path.display().to_string();
            let expanded_local = shellexpand::tilde(&local_str).into_owned();
            target.node_local_path = Some(PathBuf::from(&expanded_local));
        }

        if target.address.is_none() && !target.base_path.is_absolute() {
            return Err(ConfigError::InvalidConfig {
                detail: format!(
                    "Target '{}': `base_path` for local targets must be an absolute path or start with '~'. Got: '{}'",
                    name,
                    path_str
                ),
            });
        }
    }
    Ok(config)
}

pub fn save_config(config: &Config) -> Result<(), ConfigError> {
    let xdg_dirs = crate::xdg_dirs();
    let config_path = xdg_dirs.place_config_file(CONFIG_FILE_NAME)?;

    let toml_string = toml::to_string_pretty(config).map_err(std::io::Error::other)?;
    crate::fs_utils::write_atomic(&config_path, toml_string.as_bytes())?;
    Ok(())
}
