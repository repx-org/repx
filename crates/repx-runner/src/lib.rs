use crate::cli::{Cli, Commands};
use crate::commands::AppContext;
use repx_client::Client;
use repx_core::{
    config, constants::targets, errors::CoreError, logging::Verbosity, model::SchedulerType,
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

            let (lab_path, _lab_tar_tempdir) = if cli.lab.is_file() {
                let temp_dir =
                    tempfile::tempdir().map_err(|e| CliError::Config(CoreError::Io(e)))?;
                println!("- Extracting lab tar {} ...", cli.lab.display());
                let status = std::process::Command::new("tar")
                    .arg("xf")
                    .arg(&cli.lab)
                    .arg("-C")
                    .arg(temp_dir.path())
                    .status()
                    .map_err(|e| CliError::Config(CoreError::Io(e)))?;
                if !status.success() {
                    return Err(CliError::Config(CoreError::InvalidConfig {
                        detail: format!("Failed to extract lab tar: {}", cli.lab.display()),
                    }));
                }
                let mut entries = std::fs::read_dir(temp_dir.path())
                    .map_err(|e| CliError::Config(CoreError::Io(e)))?;
                let extracted = entries
                    .next()
                    .ok_or_else(|| {
                        CliError::Config(CoreError::InvalidConfig {
                            detail: "Lab tar is empty".to_string(),
                        })
                    })?
                    .map_err(|e| CliError::Config(CoreError::Io(e)))?
                    .path();
                (extracted, Some(temp_dir))
            } else {
                (cli.lab.clone(), None)
            };

            let client = create_client(&config, &lab_path)?;

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
                lab_path: &lab_path,
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
