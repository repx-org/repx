use crate::cli::{Cli, Commands};
use crate::commands::AppContext;
use repx_client::Client;
use repx_core::{
    config, constants::targets, errors::ConfigError, logging::Verbosity, model::SchedulerType,
};

pub mod cli;
pub mod commands;
pub mod error;

use error::CliError;

fn create_client(config: &config::Config, lab_path: &std::path::Path) -> Result<Client, CliError> {
    Client::new(config.clone(), lab_path.to_path_buf()).map_err(|e| CliError::ExecutionFailed {
        message: "Failed to initialize client".to_string(),
        log_path: None,
        log_summary: e.to_string(),
    })
}

pub fn run(cli: Cli) -> Result<(), CliError> {
    tracing::trace!(
        "repx invoked with: {:?}",
        std::env::args().collect::<Vec<_>>()
    );

    match cli.command {
        Commands::InternalOrchestrate(args) => {
            commands::internal::handle_internal_orchestrate(args)
        }
        Commands::InternalExecute(args) => commands::execute::handle_execute(args),
        Commands::InternalScatterGather(args) => {
            commands::scatter_gather::handle_scatter_gather(*args, Verbosity::from(cli.verbose))
        }
        Commands::InternalGc(args) => {
            let rt = commands::create_tokio_runtime()?;
            rt.block_on(commands::gc::async_handle_internal_gc(args))
        }
        Commands::List(args) => commands::list::handle_list(args, &cli.lab, cli.target.as_deref()),
        Commands::Show(args) => commands::show::handle_show(args, &cli.lab, cli.target.as_deref()),
        Commands::TraceParams(args) => commands::trace::handle_trace_params(args, &cli.lab),
        Commands::Log(args) => {
            let config = config::load_config()?;
            let client = create_client(&config, &cli.lab)?;
            let target_name = cli
                .target
                .as_deref()
                .or(config.submission_target.as_deref())
                .unwrap_or(targets::LOCAL);
            let context = AppContext {
                lab_path: &cli.lab,
                client: &client,
                submission_target: target_name,
            };
            commands::log::handle_log(args, &context)
        }
        Commands::Gc(args) => {
            let config = config::load_config()?;
            let client = create_client(&config, &cli.lab)?;
            let submission_target = args
                .target
                .clone()
                .or(config.submission_target.clone())
                .unwrap_or_else(|| targets::LOCAL.to_string());
            let context = AppContext {
                lab_path: &cli.lab,
                client: &client,
                submission_target: &submission_target,
            };
            commands::gc::handle_gc_dispatch(args, &context, &config, Verbosity::from(cli.verbose))
        }
        Commands::Run(args) => {
            let config = config::load_config()?;
            let resources = config::load_resources(cli.resources.as_deref())?;

            let client = create_client(&config, &cli.lab)?;

            let target_name = match cli.target.as_ref().or(config.submission_target.as_ref()) {
                Some(name) => name.clone(),
                None => return Err(CliError::Config(ConfigError::NoSubmissionTarget)),
            };

            let target_config = config.targets.get(&target_name).ok_or_else(|| {
                CliError::Config(ConfigError::TargetNotConfigured {
                    name: target_name.clone(),
                })
            })?;

            let scheduler: SchedulerType = if let Some(s) = &cli.scheduler {
                s.parse()
                    .map_err(|e: repx_core::model::ParseSchedulerTypeError| {
                        CliError::Config(ConfigError::InvalidConfig {
                            detail: e.to_string(),
                        })
                    })?
            } else {
                target_config
                    .default_scheduler
                    .or(config.default_scheduler)
                    .unwrap_or(SchedulerType::Slurm)
            };

            let num_jobs = if scheduler == SchedulerType::Local {
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
                resources,
                &target_name,
                scheduler,
                num_jobs,
                Verbosity::from(cli.verbose),
            )
        }
    }
}
