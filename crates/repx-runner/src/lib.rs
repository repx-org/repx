use crate::cli::{Cli, Commands};
use crate::commands::AppContext;
use repx_client::Client;
use repx_core::{
    config, constants::targets, errors::CoreError, lab, lab::LabSource, logging::Verbosity,
    model::SchedulerType,
};

pub mod cli;
pub mod commands;
pub mod error;

use error::CliError;

fn create_client(config: &config::Config, source: &LabSource) -> Result<Client, CliError> {
    Client::new(config.clone(), source.clone()).map_err(|e| CliError::ExecutionFailed {
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
        Commands::List(args) => {
            let source = LabSource::from_path(&cli.lab);
            let loaded_lab = lab::load(&source)?;
            commands::list::handle_list(args, &loaded_lab, &source, cli.target.as_deref())
        }
        Commands::Show(args) => {
            let source = LabSource::from_path(&cli.lab);
            let loaded_lab = lab::load(&source)?;
            commands::show::handle_show(args, &loaded_lab, &source, cli.target.as_deref())
        }
        Commands::TraceParams(args) => {
            let source = LabSource::from_path(&cli.lab);
            let loaded_lab = lab::load(&source)?;
            commands::trace::handle_trace_params(args, &loaded_lab)
        }
        Commands::Log(args) => {
            let source = LabSource::from_path(&cli.lab);
            let config = config::load_config()?;
            let client = create_client(&config, &source)?;
            let target_name = cli
                .target
                .as_deref()
                .or(config.submission_target.as_deref())
                .unwrap_or(targets::LOCAL);
            let context = AppContext {
                source: &source,
                client: &client,
                submission_target: target_name,
            };
            commands::log::handle_log(args, &context)
        }
        Commands::Gc(args) => {
            let source = LabSource::from_path(&cli.lab);
            let config = config::load_config()?;
            let client = create_client(&config, &source)?;
            let submission_target = args
                .target
                .clone()
                .or(config.submission_target.clone())
                .unwrap_or_else(|| targets::LOCAL.to_string());
            let context = AppContext {
                source: &source,
                client: &client,
                submission_target: &submission_target,
            };
            commands::gc::handle_gc_dispatch(args, &context, &config, Verbosity::from(cli.verbose))
        }
        Commands::Run(args) => {
            let source = LabSource::from_path(&cli.lab);
            let config = config::load_config()?;
            let resources = config::load_resources(cli.resources.as_deref())?;

            let client = create_client(&config, &source)?;

            let target_name = match cli.target.as_ref().or(config.submission_target.as_ref()) {
                Some(name) => name.clone(),
                None => return Err(CliError::Config(CoreError::NoSubmissionTarget)),
            };

            let target_config = config.targets.get(&target_name).ok_or_else(|| {
                CliError::Config(CoreError::TargetNotConfigured {
                    name: target_name.clone(),
                })
            })?;

            let scheduler: SchedulerType = if let Some(s) = cli.scheduler {
                s
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

            let artifact_store = args
                .artifact_store
                .map(repx_core::model::ArtifactStore::from)
                .or(target_config.artifact_store)
                .unwrap_or_default();

            let context = AppContext {
                source: &source,
                client: &client,
                submission_target: &target_name,
            };

            commands::run::handle_run(
                args,
                &context,
                resources,
                commands::run::RunConfig {
                    target_name: target_name.clone(),
                    scheduler,
                    num_jobs,
                    verbose: Verbosity::from(cli.verbose),
                    artifact_store,
                },
            )
        }
    }
}
