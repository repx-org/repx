use crate::cli::{Cli, Commands};
use crate::commands::AppContext;
use clap::Parser;
use num_cpus;
use repx_client::Client;
use repx_core::{
    config::{self, Resources},
    error::AppError,
    log_debug, log_trace,
    logging::{self, LogLevel},
};
use std::fs;
use std::path::{Path, PathBuf};
use toml::Value;
use xdg::BaseDirectories;

pub mod cli;
pub mod commands;

fn merge_toml_values(a: &mut Value, b: &Value) {
    match (a, b) {
        (Value::Table(a), Value::Table(b)) => {
            for (k, v) in b {
                merge_toml_values(a.entry(k.clone()).or_insert(v.clone()), v);
            }
        }
        (a, b) => {
            *a = b.clone();
        }
    }
}

fn load_resources_config(cli_path: Option<&PathBuf>) -> Result<Option<Resources>, AppError> {
    let mut merged_value = Value::Table(toml::map::Map::new());

    let xdg_dirs = BaseDirectories::with_prefix("repx");
    if let Some(global_path) = xdg_dirs.find_config_file("resources.toml") {
        log_debug!("Loading global resources from: {}", global_path.display());
        let content = fs::read_to_string(global_path)?;
        let global_value: Value = toml::from_str(&content).map_err(AppError::Toml)?;
        merge_toml_values(&mut merged_value, &global_value);
    }

    let cwd_path = Path::new("./resources.toml");
    if cwd_path.exists() {
        log_debug!("Loading local resources from: {}", cwd_path.display());
        let content = fs::read_to_string(cwd_path)?;
        let local_value: Value = toml::from_str(&content).map_err(AppError::Toml)?;
        merge_toml_values(&mut merged_value, &local_value);
    }

    if let Some(path) = cli_path {
        if path.exists() {
            log_debug!("Loading specified resources from: {}", path.display());
            let content = fs::read_to_string(path)?;
            let cli_value: Value = toml::from_str(&content).map_err(AppError::Toml)?;
            merge_toml_values(&mut merged_value, &cli_value);
        } else {
            return Err(AppError::PathIo {
                path: path.clone(),
                source: std::io::Error::new(std::io::ErrorKind::NotFound, "File not found"),
            });
        }
    }

    if merged_value.as_table().map_or(true, |t| t.is_empty()) {
        Ok(None)
    } else {
        let final_resources: Resources = merged_value.try_into().map_err(AppError::Toml)?;
        Ok(Some(final_resources))
    }
}

pub fn run() -> Result<(), AppError> {
    let cli = Cli::parse();
    if cli.verbose > 0 {
        logging::set_log_level(LogLevel::from(cli.verbose + 1));
    }
    log_trace!(
        "repx invoked with: {:?}",
        std::env::args().collect::<Vec<_>>()
    );

    match cli.command {
        Commands::InternalOrchestrate(args) => {
            commands::internal::handle_internal_orchestrate(args)
        }
        Commands::InternalExecute(args) => commands::execute::handle_execute(args),
        Commands::InternalScatterGather(args) => {
            commands::scatter_gather::handle_scatter_gather(args)
        }
        Commands::Run(args) => {
            let config = config::load_config()?;
            let resources = load_resources_config(cli.resources.as_ref())?;

            let client = Client::new(config.clone(), cli.lab.clone()).map_err(|e| {
                AppError::ExecutionFailed {
                    message: "Failed to initialize client".to_string(),
                    log_path: None,
                    log_summary: e.to_string(),
                }
            })?;

            let target_name = match cli.target.as_ref().or(config.submission_target.as_ref()) {
                Some(name) => name.clone(),
                None => {
                    return Err(AppError::ConfigurationError(
                        "No submission target specified. Set 'submission_target' in your config or use the --target flag.".to_string(),
                    ))
                }
            };

            let target_config = config.targets.get(&target_name).ok_or_else(|| {
                AppError::ConfigurationError(format!(
                    "Target '{}' not found in configuration.",
                    target_name
                ))
            })?;

            let scheduler = cli
                .scheduler
                .as_deref()
                .or(target_config.scheduler.as_deref())
                .or(config.default_scheduler.as_deref())
                .unwrap_or("slurm")
                .to_string();

            let num_jobs = if scheduler == "local" {
                Some(
                    args.jobs
                        .or(target_config.local_concurrency)
                        .unwrap_or_else(num_cpus::get),
                )
            } else {
                None
            };

            let context = AppContext {
                lab_path: &cli.lab,
                client: &client,
                submission_target: &target_name,
            };

            commands::run::handle_run(
                args,
                &context,
                &config,
                resources,
                &target_name,
                &scheduler,
                num_jobs,
            )
        }
    }
}
