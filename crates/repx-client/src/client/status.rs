use super::Client;
use crate::error::{ClientError, Result};
use crate::targets::SlurmState;
use repx_core::{
    engine,
    model::{JobId, RunId, SchedulerType},
};
use std::collections::{BTreeMap, HashMap};

fn cleanup_slurm_map(
    client: &Client,
    outcomes: &HashMap<JobId, engine::JobStatus>,
    target_filter: Option<&str>,
) -> Result<()> {
    let mut guard = super::lock_slurm_map(&client.slurm_map);
    let mut changed = false;
    guard.retain(|job_id, entry| {
        if let Some(target) = target_filter {
            if entry.target_name != target {
                return true;
            }
        }
        let is_done = matches!(
            outcomes.get(job_id),
            Some(engine::JobStatus::Succeeded { .. }) | Some(engine::JobStatus::Failed { .. })
        );
        if is_done {
            changed = true;
        }
        !is_done
    });
    drop(guard);
    if changed {
        client.save_slurm_map()?;
    }
    Ok(())
}

pub fn get_statuses(
    client: &Client,
) -> Result<(
    BTreeMap<RunId, engine::JobStatus>,
    HashMap<JobId, engine::JobStatus>,
)> {
    let mut job_statuses = HashMap::new();
    for target in client.targets.values() {
        let outcomes = target.check_outcome_markers()?;
        job_statuses.extend(outcomes);
    }

    cleanup_slurm_map(client, &job_statuses, None)?;

    for target in client.targets.values() {
        if target.config().slurm.is_some() {
            let queued_jobs = target.squeue()?;
            for (job_id, squeue_info) in queued_jobs {
                job_statuses
                    .entry(job_id)
                    .or_insert(if squeue_info.state == SlurmState::Running {
                        engine::JobStatus::Running
                    } else {
                        engine::JobStatus::Queued
                    });
            }
        }
    }

    let final_statuses = engine::determine_job_statuses(&client.lab, job_statuses);
    let run_statuses = engine::determine_run_aggregate_statuses(&client.lab, &final_statuses);

    Ok((run_statuses, final_statuses))
}

pub fn get_statuses_for_active_target(
    client: &Client,
    active_target_name: &str,
    active_scheduler: Option<SchedulerType>,
) -> Result<HashMap<JobId, engine::JobStatus>> {
    let mut job_statuses = HashMap::new();
    let target = client
        .targets
        .get(active_target_name)
        .ok_or_else(|| ClientError::TargetNotFound(active_target_name.to_string()))?;

    let outcomes = target.check_outcome_markers()?;
    job_statuses.extend(outcomes);

    cleanup_slurm_map(client, &job_statuses, Some(active_target_name))?;

    let has_tracked_slurm_jobs = {
        let guard = super::lock_slurm_map(&client.slurm_map);
        guard
            .values()
            .any(|entry| entry.target_name == active_target_name)
    };

    let should_query_slurm = target.config().slurm.is_some()
        && match active_scheduler {
            Some(SchedulerType::Slurm) => true,
            Some(_) => false,
            None => has_tracked_slurm_jobs,
        };

    if should_query_slurm {
        let queued_jobs = target.squeue()?;
        for (job_id, squeue_info) in queued_jobs {
            job_statuses
                .entry(job_id)
                .or_insert(if squeue_info.state == SlurmState::Running {
                    engine::JobStatus::Running
                } else {
                    engine::JobStatus::Queued
                });
        }
    }

    Ok(job_statuses)
}
