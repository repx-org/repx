use super::Client;
use crate::error::{ClientError, Result};
use crate::targets::SlurmState;
use repx_core::{
    engine,
    model::{JobId, RunId},
};
use std::collections::{BTreeMap, HashMap};

pub fn get_statuses(
    client: &Client,
) -> Result<(
    BTreeMap<RunId, engine::JobStatus>,
    HashMap<JobId, engine::JobStatus>,
)> {
    let mut all_outcomes = HashMap::new();
    for target in client.targets.values() {
        let outcomes = target.check_outcome_markers()?;
        all_outcomes.extend(outcomes);
    }

    let mut slurm_map_guard = client.slurm_map.lock().unwrap();
    let mut map_was_changed = false;
    slurm_map_guard.retain(|job_id, _| {
        let is_done = matches!(
            all_outcomes.get(job_id),
            Some(engine::JobStatus::Succeeded { .. }) | Some(engine::JobStatus::Failed { .. })
        );
        if is_done {
            map_was_changed = true;
        }
        !is_done
    });

    drop(slurm_map_guard);

    if map_was_changed {
        client.save_slurm_map()?;
    }

    let mut job_statuses = all_outcomes;

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

    let final_statuses = engine::determine_job_statuses(&client.lab, &job_statuses);
    let run_statuses = engine::determine_run_aggregate_statuses(&client.lab, &final_statuses);

    Ok((run_statuses, final_statuses))
}

pub fn get_statuses_for_active_target(
    client: &Client,
    active_target_name: &str,
) -> Result<HashMap<JobId, engine::JobStatus>> {
    let mut job_statuses = HashMap::new();
    let target = client
        .targets
        .get(active_target_name)
        .ok_or_else(|| ClientError::TargetNotFound(active_target_name.to_string()))?;

    let outcomes = target.check_outcome_markers()?;
    job_statuses.extend(outcomes.clone());

    let mut slurm_map_guard = client.slurm_map.lock().unwrap();
    let mut map_was_changed = false;
    slurm_map_guard.retain(|job_id, (target_name, _slurm_id)| {
        if target_name != active_target_name {
            return true;
        }
        let is_done = matches!(
            outcomes.get(job_id),
            Some(engine::JobStatus::Succeeded { .. }) | Some(engine::JobStatus::Failed { .. })
        );
        if is_done {
            map_was_changed = true;
        }
        !is_done
    });

    drop(slurm_map_guard);

    if map_was_changed {
        client.save_slurm_map()?;
    }

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

    Ok(job_statuses)
}
