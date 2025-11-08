use crate::error::AppError;
use crate::theme;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;
use xdg::BaseDirectories;

const CONFIG_FILE_NAME: &str = "config.toml";
const THEME_FILE_NAME: &str = "theme.toml";
const RESOURCES_FILE_NAME: &str = "resources.toml";

const DEFAULT_CONFIG_CONTENT: &str = r#"# Repx Configuration File
# This file was generated automatically. You can edit it to customize Repx's behavior.

# The theme for the command-line interface.
# A custom theme can be defined in `theme.toml` in the same directory.
theme = "default"

# The default target to use for `repx run` if not specified via --target.
# This must match one of the names in the [targets] section below.
submission_target = "local"

# The default scheduler to use if not specified in a target or via the CLI.
# Can be "slurm" or "local".
default_scheduler = "slurm"

# --- Execution Targets ---
# Defines the machines (local or remote) where jobs can be submitted.
[targets]

  # The 'local' target runs jobs on your current machine.
  [targets.local]
  # The base path for the shared path on the target. Tilde expansion (~) is supported.
  base_path = "~/Desktop/repx-store"
  # The scheduler to use for this target. For 'local' targets, this can be
  # "slurm" (if you have SLURM client tools) or "local" (to run directly).
  scheduler = "local"
  # The maximum number of jobs to run in parallel on this machine.
  # If not set, it defaults to the number of available CPU cores.
  # local_concurrency = 4
  # Execution type. Can be "native", "podman", "docker", or "bwrap".
  # If unset, defaults to "native".
  # The corresponding runtime must be installed on the target machine.
  execution_type = "native"

  # Example of a remote SSH target for a SLURM HPC cluster.
  # [targets.safari]
  # # The SSH connection string.
  # address = "safari"
  # # The base path for the shared path on the target.
  # base_path = "/mnt/galactica/demirlie/Desktop/repx-store"
  # # This target uses the SLURM scheduler.
  # scheduler = "slurm"
  # # Use podman for execution on this target.
  # execution_type = "podman"
"#;

const DEFAULT_RESOURCES_CONTENT: &str = r#"# Repx Resource Configuration File
# This file allows you to specify SLURM resource requirements for your jobs.
# Repx applies these rules by matching against the job ID and the target name.

# The `[defaults]` section applies to all jobs unless overridden by a specific rule.
[defaults]
# partition = "compute"
# cpus-per-task = 1
# mem = "4G"
# time = "01:00:00" # 1 hour
# sbatch_opts = ["--gres=gpu:1"] # Custom SBATCH options

# The `[[rules]]` array defines specific overrides. Rules are applied in order,
# with later matching rules overwriting earlier ones.
[[rules]]
# Example: Match any job whose ID contains "-heavy-" and run it on a specific partition.
# job_id_glob = "*-heavy-*"
# partition = "high-mem"
# mem = "64G"
# time = "24:00:00"

[[rules]]
# Example: Override resources for a scatter-gather orchestrator AND its workers.
# job_id_glob = "*-stage-champsim-trace" # Matches the orchestrator job
# mem = "1G" # The orchestrator is lightweight
# time = "00:10:00"
#
#   # This special nested table defines resources for the parallel WORKER jobs.
#   # If this is omitted, workers will inherit the orchestrator's settings (1G mem, 10min time).
#   [rules.worker_resources]
#   mem = "8G"
#   cpus-per-task = 1
#   time = "02:00:00"
"#;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub address: Option<String>,
    pub base_path: PathBuf,
    pub scheduler: Option<String>,
    pub execution_type: Option<String>,
    pub local_concurrency: Option<usize>,
}

const TUI_DEFAULT_TICK_RATE_MS: u64 = 1000;

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub theme: Option<String>,
    pub submission_target: Option<String>,
    pub default_scheduler: Option<String>,
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

fn create_default_config_if_missing(xdg_dirs: &BaseDirectories) -> Result<PathBuf, AppError> {
    match xdg_dirs.find_config_file(CONFIG_FILE_NAME) {
        Some(path) => Ok(path),
        None => {
            let config_path = xdg_dirs.place_config_file(CONFIG_FILE_NAME)?;
            fs::write(&config_path, DEFAULT_CONFIG_CONTENT)?;
            Ok(config_path)
        }
    }
}

fn create_default_theme_if_missing(xdg_dirs: &BaseDirectories) -> Result<(), AppError> {
    if xdg_dirs.find_config_file(THEME_FILE_NAME).is_none() {
        let theme_path = xdg_dirs.place_config_file(THEME_FILE_NAME)?;
        let default_theme = theme::default_theme();
        let theme_toml = toml::to_string_pretty(&default_theme)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        fs::write(theme_path, theme_toml)?;
    }
    Ok(())
}

fn create_default_resources_if_missing(xdg_dirs: &BaseDirectories) -> Result<(), AppError> {
    if xdg_dirs.find_config_file(RESOURCES_FILE_NAME).is_none() {
        let resources_path = xdg_dirs.place_config_file(RESOURCES_FILE_NAME)?;
        fs::write(resources_path, DEFAULT_RESOURCES_CONTENT)?;
    }
    Ok(())
}

pub fn load_config() -> Result<Config, AppError> {
    let xdg_dirs = BaseDirectories::with_prefix("repx");

    let config_path = create_default_config_if_missing(&xdg_dirs)?;
    create_default_theme_if_missing(&xdg_dirs)?;
    create_default_resources_if_missing(&xdg_dirs)?;

    let file_content = fs::read_to_string(config_path)?;
    let mut config: Config = toml::from_str(&file_content)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    for (name, target) in config.targets.iter_mut() {
        let path_str = target.base_path.display().to_string();
        let expanded_path_str = shellexpand::tilde(&path_str).into_owned();
        target.base_path = PathBuf::from_str(&expanded_path_str).map_err(|e| {
            AppError::ConfigurationError(format!("Invalid path '{}': {}", expanded_path_str, e))
        })?;

        if target.address.is_none() && !target.base_path.is_absolute() {
            return Err(AppError::ConfigurationError(format!(
                "Target '{}': `base_path` for local targets must be an absolute path or start with '~'. Got: '{}'",
                name,
                path_str
            )));
        }
    }
    Ok(config)
}

pub fn save_config(config: &Config) -> Result<(), AppError> {
    let xdg_dirs = BaseDirectories::with_prefix("repx");
    let config_path = xdg_dirs.place_config_file(CONFIG_FILE_NAME)?;

    let toml_string = toml::to_string_pretty(config)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    fs::write(config_path, toml_string)?;
    Ok(())
}
