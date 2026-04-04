use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use repx_client::{ClientEvent, SubmitOptions, WorkUnitPhase};
use repx_core::{config::Resources, errors::CoreError, model::SchedulerType};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::thread;
use std::time::Duration;

use crate::{cli::RunArgs, commands::AppContext, error::CliError};
use repx_core::model::Memory;

pub struct RunConfig {
    pub target_name: String,
    pub scheduler: SchedulerType,
    pub num_jobs: Option<usize>,
    pub verbose: repx_core::logging::Verbosity,
    pub artifact_store: repx_core::model::ArtifactStore,
}

fn format_phase_suffix(phase: &Option<WorkUnitPhase>) -> String {
    match phase {
        Some(p) => format!(" [{}]", p).dimmed().to_string(),
        None => String::new(),
    }
}

fn format_wall_time(d: &Duration) -> String {
    let total_secs = d.as_secs();
    let days = total_secs / 86400;
    let hours = (total_secs % 86400) / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;

    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("{}d", days));
    }
    if hours > 0 {
        parts.push(format!("{}h", hours));
    }
    if mins > 0 {
        parts.push(format!("{}m", mins));
    }
    if secs > 0 || parts.is_empty() {
        parts.push(format!("{}s", secs));
    }
    parts.join(" ")
}

fn format_wall_time_suffix(wall_time: &Option<Duration>, show_timing: bool) -> String {
    if !show_timing {
        return String::new();
    }
    match wall_time {
        Some(d) => format!(" {}", format_wall_time(d)).dimmed().to_string(),
        None => String::new(),
    }
}

#[allow(clippy::expect_used)]
pub fn handle_run(
    args: RunArgs,
    context: &AppContext<'_>,
    resources: Option<Resources>,
    run_config: RunConfig,
) -> Result<(), CliError> {
    let target_name = &run_config.target_name;
    let scheduler = run_config.scheduler;
    let num_jobs = run_config.num_jobs;
    let verbose = run_config.verbose;
    let artifact_store = run_config.artifact_store;
    let mem_override = if let Some(ref mem_str) = args.mem {
        let m = Memory::from(mem_str.as_str());
        Some(m.to_bytes().ok_or_else(|| {
            CliError::Config(CoreError::InvalidConfig {
                detail: format!(
                    "Invalid memory value: '{}'. Use e.g. 32G, 128G, 512M",
                    mem_str
                ),
            })
        })?)
    } else {
        None
    };

    println!(
        "- Submitting run request to target '{}' using '{}' scheduler...",
        target_name.cyan(),
        scheduler.to_string().cyan()
    );

    let (tx, rx) = mpsc::channel();
    let client = context.client.clone();
    let run_specs = if args.run_specs.is_empty() {
        return Err(CliError::Config(CoreError::MissingArgument {
            argument: "run_specs".to_string(),
            context: "No run or job specified to run".to_string(),
        }));
    } else {
        args.run_specs
    };

    let cancelled = Arc::new(AtomicBool::new(false));
    let cancelled_clone = cancelled.clone();
    let cancelled_for_submit = cancelled.clone();
    let _ = ctrlc::set_handler(move || {
        eprintln!("\nCancellation requested (Ctrl+C). Killing running processes...");
        cancelled_clone.store(true, Ordering::SeqCst);
    });

    let target_name_clone = target_name.to_string();
    let continue_on_failure = args.continue_on_failure;

    let submission_thread = thread::spawn(move || {
        let options = SubmitOptions {
            execution_type: None,
            resources,
            num_jobs,
            mem_override,
            event_sender: Some(tx),
            continue_on_failure,
            verbose,
            cancel_flag: Some(cancelled_for_submit),
            artifact_store,
        };
        client.submit_batch_run(run_specs, &target_name_clone, scheduler, options)
    });
    let show_timing = !args.no_timing;
    let mut pb: Option<ProgressBar> = None;
    let mut user_cancelled = false;

    loop {
        if cancelled.load(Ordering::SeqCst) {
            if let Some(pb) = pb.take() {
                pb.abandon_with_message("Cancelled");
            }
            user_cancelled = true;
            break;
        }
        let event = match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(ev) => ev,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        match event {
            ClientEvent::DeployingBinary => {
                println!("- Deploying repx binary...");
            }
            ClientEvent::CreatingLabTar => {
                println!("- Creating lab tar for node-local extraction...");
            }
            ClientEvent::SyncingLabTar => {
                println!("- Syncing lab tar to target...");
            }
            ClientEvent::CheckingJobStatuses => {
                println!("- Checking job statuses on target...");
            }
            ClientEvent::PreparingInputs { num_jobs } => {
                println!("- Preparing inputs for {} jobs...", num_jobs);
            }
            ClientEvent::PreparingInputProgress {
                job_id,
                current,
                total,
            } => {
                println!(
                    "  {} {}",
                    format!("[{}/{}]", current, total).dimmed(),
                    job_id.to_string().dimmed(),
                );
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
                        .expect("static progress bar template must be valid")
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
            ClientEvent::SubmittingJobs { total, concurrency } => {
                let executor = if scheduler == SchedulerType::Slurm {
                    "SLURM"
                } else {
                    "local executor"
                };
                match concurrency {
                    Some(c) => println!(
                        "- Scheduling {} jobs via {} ({} parallel)...",
                        total.to_string().bold(),
                        executor,
                        c.to_string().bold()
                    ),
                    None => println!(
                        "- Scheduling {} jobs via {}...",
                        total.to_string().bold(),
                        executor
                    ),
                }
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
                phase,
            } => {
                let phase_suffix = format_phase_suffix(&phase);
                println!(
                    "  {} [{}/{}] {}{} (PID {})",
                    ">>".cyan(),
                    current,
                    total,
                    job_id.to_string().dimmed(),
                    phase_suffix,
                    pid,
                );
            }
            ClientEvent::JobSucceeded {
                job_id,
                phase,
                wall_time,
            } => {
                let phase_suffix = format_phase_suffix(&phase);
                let time_suffix = format_wall_time_suffix(&wall_time, show_timing);
                println!(
                    "  {} {}{}{}",
                    "OK".green().bold(),
                    job_id.to_string().dimmed(),
                    phase_suffix,
                    time_suffix,
                );
            }
            ClientEvent::JobFailed {
                job_id,
                phase,
                wall_time,
            } => {
                let phase_suffix = format_phase_suffix(&phase);
                let time_suffix = format_wall_time_suffix(&wall_time, show_timing);
                println!(
                    "  {} {}{}{}",
                    "FAIL".red().bold(),
                    job_id.to_string().dimmed(),
                    phase_suffix,
                    time_suffix,
                );
            }
            ClientEvent::JobBlocked {
                job_id,
                blocked_by,
                phase,
            } => {
                let phase_suffix = format_phase_suffix(&phase);
                tracing::debug!(
                    "Job {}{} blocked by failed dependency {}",
                    job_id,
                    phase_suffix,
                    blocked_by,
                );
            }
            ClientEvent::LocalProgress {
                running,
                succeeded,
                failed,
                blocked,
                pending,
                total,
            } => {
                let mut parts = Vec::new();

                if succeeded > 0 {
                    parts.push(format!(
                        "{} {}",
                        format!("{}/{}", succeeded, total).green().bold(),
                        "ok".green(),
                    ));
                }
                if failed > 0 {
                    parts.push(format!(
                        "{} {}",
                        format!("{}/{}", failed, total).red().bold(),
                        "fail".red(),
                    ));
                }
                if blocked > 0 {
                    parts.push(format!(
                        "{} {}",
                        format!("{}/{}", blocked, total).dimmed(),
                        "blocked".dimmed(),
                    ));
                }
                if running > 0 {
                    parts.push(format!(
                        "{} {}",
                        format!("{}", running).yellow().bold(),
                        "running".yellow(),
                    ));
                }
                if pending > 0 {
                    parts.push(format!(
                        "{} {}",
                        format!("{}", pending).dimmed(),
                        "pending".dimmed(),
                    ));
                }

                println!("  {} {}", "---".dimmed(), parts.join(" | "));
            }
            ClientEvent::WaveCompleted { wave, num_jobs } => {
                println!("- Wave {} completed ({} jobs finished).", wave, num_jobs);
            }
        }
    }

    match submission_thread.join() {
        Ok(Ok(message)) => {
            if !user_cancelled {
                println!("{}", message);
            }
        }
        Ok(Err(e)) => {
            if !user_cancelled {
                return Err(CliError::ExecutionFailed {
                    message: "Failed to submit run".to_string(),
                    log_path: None,
                    log_summary: e.to_string(),
                });
            }
        }
        Err(_panic) => {
            return Err(CliError::ExecutionFailed {
                message: "Submission thread panicked".to_string(),
                log_path: None,
                log_summary: "Internal error: submission thread panicked unexpectedly".to_string(),
            });
        }
    }

    if user_cancelled {
        eprintln!("{}", "Run cancelled by user.".red().bold());
        return Err(CliError::ExecutionFailed {
            message: "Run cancelled by user".to_string(),
            log_path: None,
            log_summary: "Ctrl+C received during submission".to_string(),
        });
    }

    Ok(())
}
