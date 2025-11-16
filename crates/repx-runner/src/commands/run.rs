use crate::{cli::RunArgs, commands::AppContext};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use repx_client::ClientEvent;
use repx_core::{
    config::{Config, Resources},
    error::AppError,
};
use std::sync::mpsc;
use std::thread;

pub fn handle_run(
    args: RunArgs,
    context: &AppContext<'_>,
    _config: &Config,
    resources: Option<Resources>,
    target_name: &str,
    scheduler: &str,
    num_jobs: Option<usize>,
) -> Result<(), AppError> {
    println!(
        "- Submitting run request to target '{}' using '{}' scheduler...",
        target_name.cyan(),
        scheduler.cyan()
    );

    let (tx, rx) = mpsc::channel();
    let client = context.client.clone();
    let run_specs = if args.run_specs.is_empty() {
        return Err(AppError::ConfigurationError(
            "No run or job specified to run.".to_string(),
        ));
    } else {
        args.run_specs
    };

    let target_name_clone = target_name.to_string();
    let scheduler_clone = scheduler.to_string();

    let submission_thread = thread::spawn(move || {
        client.submit_batch_run(
            run_specs,
            &target_name_clone,
            &scheduler_clone,
            None,
            resources,
            num_jobs,
            Some(tx),
        )
    });

    let mut pb: Option<ProgressBar> = None;

    for event in rx {
        match event {
            ClientEvent::DeployingBinary => {
                println!("- Deploying repx binary...");
            }
            ClientEvent::GeneratingSlurmScripts { num_jobs } => {
                println!("- Generating {} SLURM scripts...", num_jobs);
            }
            ClientEvent::ExecutingOrchestrator => {
                println!("- Executing orchestrator on target...");
            }
            ClientEvent::SyncingArtifacts { total } => {
                let new_pb = ProgressBar::new(total);
                new_pb
                    .set_style(
                        ProgressStyle::default_bar()
                        .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({percent}%) {msg}")
                        .unwrap()
                        .progress_chars("#>-"),
                    );
                new_pb.set_message("Syncing artifacts...");
                pb = Some(new_pb);
            }
            ClientEvent::SyncingArtifactProgress { path } => {
                if let Some(pb) = pb.as_ref() {
                    pb.inc(1);
                    pb.set_message(format!("{}", path.display()));
                }
            }
            ClientEvent::SyncingFinished => {
                if let Some(pb) = pb.as_ref() {
                    pb.finish_with_message("Sync complete");
                }
                pb = None;
            }
            ClientEvent::SubmittingJobs { total } => {
                println!(
                    "- Submitting {} jobs to {}...",
                    total,
                    if scheduler == "slurm" {
                        "SLURM"
                    } else {
                        "local executor"
                    }
                );
            }
            ClientEvent::JobSubmitted {
                job_id,
                slurm_id,
                total,
                current,
            } => {
                println!(
                    "  [{}/{}] Submitted job {} as SLURM ID {}",
                    current, total, job_id, slurm_id
                );
            }
            ClientEvent::JobStarted {
                job_id,
                pid,
                total,
                current,
            } => {
                println!(
                    "  [{}/{}] Started job {} as PID {}",
                    current, total, job_id, pid
                );
            }
            ClientEvent::WaveCompleted { wave, num_jobs } => {
                println!("- Wave {} completed ({} jobs finished).", wave, num_jobs);
            }
        }
    }

    match submission_thread.join().unwrap() {
        Ok(message) => {
            println!("{}", message.green());
        }
        Err(e) => {
            return Err(AppError::ExecutionFailed {
                message: "Failed to submit run request".to_string(),
                log_path: None,
                log_summary: e.to_string(),
            });
        }
    }

    Ok(())
}
