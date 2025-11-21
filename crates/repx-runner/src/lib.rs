use crate::cli::{Cli, Commands};
use crate::commands::AppContext;
use clap::Parser;
use repx_client::Client;
use repx_core::{
    config,
    error::AppError,
    log_trace,
    logging::{self, LogLevel},
};

pub mod cli;
pub mod commands;

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
            let resources = config::load_resources(cli.resources.as_ref())?;

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
                .or(target_config.default_scheduler.as_deref())
                .or(config.default_scheduler.as_deref())
                .unwrap_or("slurm")
                .to_string();

            let num_jobs = if scheduler == "local" {
                Some(
                    args.jobs
                        .or_else(|| {
                            target_config
                                .local
                                .as_ref()
                                .and_then(|c| c.local_concurrency)
                        })
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
