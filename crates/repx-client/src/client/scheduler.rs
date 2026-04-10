use repx_core::model::JobId;
use std::collections::{HashMap, HashSet};

#[derive(Debug, PartialEq, Eq)]
pub struct ScheduleResult {
    pub succeeded: Vec<JobId>,
    pub failed: Vec<(JobId, String)>,
    pub skipped: Vec<JobId>,
    pub waves: Vec<HashSet<JobId>>,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum SchedulerError {
    #[error("A cycle was detected in the job dependency graph. Remaining jobs: {0:?}")]
    CycleDetected(Vec<String>),
    #[error("Job failed: {0}")]
    JobFailed(String),
}

pub fn compute_topological_waves(
    graph: &HashMap<JobId, Vec<JobId>>,
) -> Result<Vec<Vec<JobId>>, SchedulerError> {
    let mut waves: Vec<Vec<JobId>> = Vec::new();
    let mut assigned: HashSet<JobId> = HashSet::new();
    let mut remaining: HashSet<JobId> = graph.keys().cloned().collect();

    while !remaining.is_empty() {
        let mut wave: Vec<JobId> = remaining
            .iter()
            .filter(|job_id| {
                let deps = graph.get(*job_id).map(|d| d.as_slice()).unwrap_or_default();
                deps.iter().all(|dep| assigned.contains(dep))
            })
            .cloned()
            .collect();

        if wave.is_empty() {
            let mut remaining_sorted: Vec<String> =
                remaining.iter().map(|j| j.to_string()).collect();
            remaining_sorted.sort();
            return Err(SchedulerError::CycleDetected(remaining_sorted));
        }

        wave.sort();
        for id in &wave {
            remaining.remove(id);
            assigned.insert(id.clone());
        }
        waves.push(wave);
    }

    Ok(waves)
}

pub fn run_wave_schedule<F>(
    graph: &HashMap<JobId, Vec<JobId>>,
    pre_completed: &HashSet<JobId>,
    continue_on_failure: bool,
    executor: &F,
) -> Result<ScheduleResult, SchedulerError>
where
    F: Fn(&JobId) -> Result<(), String>,
{
    let mut completed: HashSet<JobId> = pre_completed.clone();
    let mut failed_ids: HashSet<JobId> = HashSet::new();
    let mut failed: Vec<(JobId, String)> = Vec::new();
    let mut succeeded: Vec<JobId> = Vec::new();
    let mut waves: Vec<HashSet<JobId>> = Vec::new();

    let mut jobs_left: HashSet<JobId> = graph
        .keys()
        .filter(|id| !pre_completed.contains(*id))
        .cloned()
        .collect();

    loop {
        if jobs_left.is_empty() {
            break;
        }

        let ready: Vec<JobId> = jobs_left
            .iter()
            .filter(|job_id| {
                let deps = graph.get(*job_id).map(|d| d.as_slice()).unwrap_or_default();
                let deps_met = deps.iter().all(|dep| completed.contains(dep));
                let no_failed_deps = deps.iter().all(|dep| !failed_ids.contains(dep));
                deps_met && no_failed_deps
            })
            .cloned()
            .collect();

        if ready.is_empty() {
            if !failed_ids.is_empty() {
                break;
            }
            let mut remaining: Vec<String> = jobs_left.iter().map(|j| j.to_string()).collect();
            remaining.sort();
            return Err(SchedulerError::CycleDetected(remaining));
        }

        let mut current_wave = HashSet::new();

        for job_id in ready {
            jobs_left.remove(&job_id);

            match executor(&job_id) {
                Ok(()) => {
                    completed.insert(job_id.clone());
                    succeeded.push(job_id.clone());
                    current_wave.insert(job_id);
                }
                Err(msg) => {
                    if continue_on_failure {
                        failed_ids.insert(job_id.clone());
                        failed.push((job_id.clone(), msg));
                        current_wave.insert(job_id);
                    } else {
                        return Err(SchedulerError::JobFailed(msg));
                    }
                }
            }
        }

        if !current_wave.is_empty() {
            waves.push(current_wave);
        }
    }

    let skipped: Vec<JobId> = jobs_left.into_iter().collect();

    Ok(ScheduleResult {
        succeeded,
        failed,
        skipped,
        waves,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! job_id {
        ($name:expr) => {
            JobId::from($name)
        };
    }

    macro_rules! graph {
        ( $( $job:expr => [ $( $dep:expr ),* ] ),* $(,)? ) => {
            HashMap::from([
                $(
                    (job_id!($job), vec![$(job_id!($dep)),*]),
                )*
            ])
        };
    }

    fn success_executor(_job_id: &JobId) -> Result<(), String> {
        Ok(())
    }

    fn failing_executor(fail_set: &HashSet<JobId>) -> impl Fn(&JobId) -> Result<(), String> + '_ {
        move |job_id: &JobId| {
            if fail_set.contains(job_id) {
                Err(format!("{} failed", job_id.as_str()))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn test_simple_linear_chain() {
        let graph = graph! {
            "A" => [],
            "B" => ["A"],
            "C" => ["B"],
        };
        let result = run_wave_schedule(&graph, &HashSet::new(), false, &success_executor)
            .expect("schedule should succeed");

        assert_eq!(result.waves.len(), 3);
        assert!(result.waves[0].contains(&job_id!("A")));
        assert!(result.waves[1].contains(&job_id!("B")));
        assert!(result.waves[2].contains(&job_id!("C")));
        assert_eq!(result.succeeded.len(), 3);
    }

    #[test]
    fn test_simple_fan_out() {
        let graph = graph! {
            "A" => [],
            "B" => ["A"],
            "C" => ["A"],
        };
        let result = run_wave_schedule(&graph, &HashSet::new(), false, &success_executor)
            .expect("schedule should succeed");

        assert_eq!(result.waves.len(), 2);
        assert!(result.waves[0].contains(&job_id!("A")));
        assert!(result.waves[1].contains(&job_id!("B")));
        assert!(result.waves[1].contains(&job_id!("C")));
    }

    #[test]
    fn test_simple_fan_in() {
        let graph = graph! {
            "A" => [],
            "B" => [],
            "C" => ["A", "B"],
        };
        let result = run_wave_schedule(&graph, &HashSet::new(), false, &success_executor)
            .expect("schedule should succeed");

        assert_eq!(result.waves.len(), 2);
        assert!(result.waves[0].contains(&job_id!("A")));
        assert!(result.waves[0].contains(&job_id!("B")));
        assert!(result.waves[1].contains(&job_id!("C")));
    }

    #[test]
    fn test_complex_dag() {
        let graph = graph! {
            "A" => [],
            "B" => ["A"],
            "C" => ["A"],
            "D" => ["B", "C"],
            "E" => ["C"],
        };
        let result = run_wave_schedule(&graph, &HashSet::new(), false, &success_executor)
            .expect("schedule should succeed");

        assert_eq!(result.waves.len(), 3);
        assert!(result.waves[0].contains(&job_id!("A")));
        assert!(result.waves[1].contains(&job_id!("B")));
        assert!(result.waves[1].contains(&job_id!("C")));
        assert!(result.waves[2].contains(&job_id!("D")));
        assert!(result.waves[2].contains(&job_id!("E")));
    }

    #[test]
    fn test_disconnected_graphs() {
        let graph = graph! {
            "A" => [],
            "B" => ["A"],
            "X" => [],
            "Y" => ["X"],
        };
        let result = run_wave_schedule(&graph, &HashSet::new(), false, &success_executor)
            .expect("schedule should succeed");

        assert_eq!(result.waves.len(), 2);
        assert!(result.waves[0].contains(&job_id!("A")));
        assert!(result.waves[0].contains(&job_id!("X")));
        assert!(result.waves[1].contains(&job_id!("B")));
        assert!(result.waves[1].contains(&job_id!("Y")));
    }

    #[test]
    fn test_graph_with_pre_completed_dependency() {
        let graph = graph! {
            "A" => [],
            "C" => ["A"],
            "D" => ["B", "C"],
            "E" => ["C"],
        };

        let pre_completed: HashSet<JobId> = [job_id!("B")].into_iter().collect();
        let result = run_wave_schedule(&graph, &pre_completed, false, &success_executor)
            .expect("schedule should succeed");

        assert_eq!(result.waves.len(), 3);
        assert!(result.waves[0].contains(&job_id!("A")));
        assert!(result.waves[1].contains(&job_id!("C")));
        assert!(result.waves[2].contains(&job_id!("D")));
        assert!(result.waves[2].contains(&job_id!("E")));
    }

    #[test]
    fn test_cycle_detection() {
        let graph = graph! {
            "A" => ["C"],
            "B" => ["A"],
            "C" => ["B"],
        };
        let result = run_wave_schedule(&graph, &HashSet::new(), false, &success_executor);

        assert!(result.is_err());
        match result.expect_err("schedule should fail") {
            SchedulerError::CycleDetected(remaining) => {
                assert_eq!(
                    remaining,
                    vec!["A".to_string(), "B".to_string(), "C".to_string()]
                );
            }
            other => panic!("Expected CycleDetected, got {:?}", other),
        }
    }

    #[test]
    fn test_empty_input_graph() {
        let graph = graph! {};
        let result = run_wave_schedule(&graph, &HashSet::new(), false, &success_executor)
            .expect("schedule should succeed");

        assert!(result.waves.is_empty());
        assert!(result.succeeded.is_empty());
    }

    #[test]
    fn test_diamond_dependency() {
        let graph = graph! {
            "start" => [],
            "mid_a" => ["start"],
            "mid_b" => ["start"],
            "end"   => ["mid_a", "mid_b"],
        };
        let result = run_wave_schedule(&graph, &HashSet::new(), false, &success_executor)
            .expect("schedule should succeed");

        assert_eq!(result.waves.len(), 3);
        assert!(result.waves[0].contains(&job_id!("start")));
        assert!(result.waves[1].contains(&job_id!("mid_a")));
        assert!(result.waves[1].contains(&job_id!("mid_b")));
        assert!(result.waves[2].contains(&job_id!("end")));
    }

    #[test]
    fn test_shared_sub_graph() {
        let graph = graph! {
            "top_a" => [],
            "top_b" => [],
            "independent_leaf" => ["top_a"],
            "shared_mid" => ["top_a", "top_b"],
            "shared_leaf" => ["shared_mid"],
        };
        let result = run_wave_schedule(&graph, &HashSet::new(), false, &success_executor)
            .expect("schedule should succeed");

        assert_eq!(result.waves.len(), 3);
        assert!(result.waves[0].contains(&job_id!("top_a")));
        assert!(result.waves[0].contains(&job_id!("top_b")));
        assert!(result.waves[1].contains(&job_id!("independent_leaf")));
        assert!(result.waves[1].contains(&job_id!("shared_mid")));
        assert!(result.waves[2].contains(&job_id!("shared_leaf")));
    }

    #[test]
    fn test_multiple_independent_diamond_graphs() {
        let graph = graph! {
            "start_a" => [],
            "mid_a1"  => ["start_a"],
            "mid_a2"  => ["start_a"],
            "end_a"   => ["mid_a1", "mid_a2"],

            "start_b" => [],
            "mid_b1"  => ["start_b"],
            "mid_b2"  => ["start_b"],
            "end_b"   => ["mid_b1", "mid_b2"],

            "start_c" => [],
            "mid_c1"  => ["start_c"],
            "mid_c2"  => ["start_c"],
            "end_c"   => ["mid_c1", "mid_c2"],
        };
        let result = run_wave_schedule(&graph, &HashSet::new(), false, &success_executor)
            .expect("schedule should succeed");

        assert_eq!(result.waves.len(), 3);
        for s in ["start_a", "start_b", "start_c"] {
            assert!(result.waves[0].contains(&job_id!(s)));
        }
        for m in ["mid_a1", "mid_a2", "mid_b1", "mid_b2", "mid_c1", "mid_c2"] {
            assert!(result.waves[1].contains(&job_id!(m)));
        }
        for e in ["end_a", "end_b", "end_c"] {
            assert!(result.waves[2].contains(&job_id!(e)));
        }
    }

    #[test]
    fn test_intertwined_graphs_with_shared_dependency() {
        let graph = graph! {
            "start_a" => [],
            "start_b" => [],
            "mid_a1" => ["start_a"],
            "mid_b1" => ["start_b"],
            "mid_a2" => ["start_a"],
            "end_a" => ["mid_a1", "mid_a2"],
            "end_b" => ["mid_b1", "mid_a2"],
        };

        let result = run_wave_schedule(&graph, &HashSet::new(), false, &success_executor)
            .expect("schedule should succeed");

        assert_eq!(result.waves.len(), 3);
        assert!(result.waves[0].contains(&job_id!("start_a")));
        assert!(result.waves[0].contains(&job_id!("start_b")));
        assert!(result.waves[1].contains(&job_id!("mid_a1")));
        assert!(result.waves[1].contains(&job_id!("mid_b1")));
        assert!(result.waves[1].contains(&job_id!("mid_a2")));
        assert!(result.waves[2].contains(&job_id!("end_a")));
        assert!(result.waves[2].contains(&job_id!("end_b")));
    }

    #[test]
    fn test_fail_fast_stops_on_first_failure() {
        let graph = graph! {
            "A" => [],
            "B" => [],
            "C" => ["A", "B"],
        };
        let fail_set: HashSet<JobId> = [job_id!("A")].into_iter().collect();
        let result =
            run_wave_schedule(&graph, &HashSet::new(), false, &failing_executor(&fail_set));

        assert!(result.is_err());
        match result.expect_err("schedule should fail") {
            SchedulerError::JobFailed(msg) => assert!(msg.contains("A failed")),
            other => panic!("Expected JobFailed, got {:?}", other),
        }
    }

    #[test]
    fn test_fail_fast_all_succeed() {
        let graph = graph! {
            "A" => [],
            "B" => ["A"],
            "C" => ["B"],
        };
        let result = run_wave_schedule(&graph, &HashSet::new(), false, &success_executor)
            .expect("schedule should succeed");

        assert_eq!(result.failed.len(), 0);
        assert_eq!(result.skipped.len(), 0);
        assert_eq!(result.succeeded.len(), 3);
    }

    #[test]
    fn test_continue_on_failure_runs_independent_jobs() {
        let graph = graph! {
            "A" => [],
            "B" => ["A"],
            "X" => [],
            "Y" => ["X"],
        };
        let fail_set: HashSet<JobId> = [job_id!("A")].into_iter().collect();
        let result = run_wave_schedule(&graph, &HashSet::new(), true, &failing_executor(&fail_set))
            .expect("schedule should succeed");

        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].0, job_id!("A"));

        assert!(result.skipped.contains(&job_id!("B")));

        assert!(result.succeeded.contains(&job_id!("X")));
        assert!(result.succeeded.contains(&job_id!("Y")));
    }

    #[test]
    fn test_continue_on_failure_reports_multiple_failures() {
        let graph = graph! {
            "A" => [],
            "B" => [],
            "C" => [],
        };
        let fail_set: HashSet<JobId> = [job_id!("A"), job_id!("B")].into_iter().collect();
        let result = run_wave_schedule(&graph, &HashSet::new(), true, &failing_executor(&fail_set))
            .expect("schedule should succeed");

        assert_eq!(result.failed.len(), 2);
        let failed_ids: HashSet<_> = result.failed.iter().map(|(id, _)| id.clone()).collect();
        assert!(failed_ids.contains(&job_id!("A")));
        assert!(failed_ids.contains(&job_id!("B")));
        assert!(result.succeeded.contains(&job_id!("C")));
        assert_eq!(result.skipped.len(), 0);
    }

    #[test]
    fn test_continue_on_failure_cascading_skips() {
        let graph = graph! {
            "A" => [],
            "B" => ["A"],
            "C" => ["B"],
            "D" => ["C"],
        };
        let fail_set: HashSet<JobId> = [job_id!("A")].into_iter().collect();
        let result = run_wave_schedule(&graph, &HashSet::new(), true, &failing_executor(&fail_set))
            .expect("schedule should succeed");

        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].0, job_id!("A"));

        assert_eq!(result.skipped.len(), 3);
        assert!(result.skipped.contains(&job_id!("B")));
        assert!(result.skipped.contains(&job_id!("C")));
        assert!(result.skipped.contains(&job_id!("D")));
    }

    #[test]
    fn test_continue_on_failure_diamond_with_one_path_failing() {
        let graph = graph! {
            "start" => [],
            "mid_a" => ["start"],
            "mid_b" => ["start"],
            "end" => ["mid_a", "mid_b"],
        };
        let fail_set: HashSet<JobId> = [job_id!("mid_a")].into_iter().collect();
        let result = run_wave_schedule(&graph, &HashSet::new(), true, &failing_executor(&fail_set))
            .expect("schedule should succeed");

        assert!(result.succeeded.contains(&job_id!("start")));
        assert!(result.succeeded.contains(&job_id!("mid_b")));
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].0, job_id!("mid_a"));
        assert!(result.skipped.contains(&job_id!("end")));
    }

    #[test]
    fn test_continue_on_failure_partial_fan_out_failure() {
        let graph = graph! {
            "start" => [],
            "A" => ["start"],
            "B" => ["start"],
            "C" => ["start"],
        };
        let fail_set: HashSet<JobId> = [job_id!("B")].into_iter().collect();
        let result = run_wave_schedule(&graph, &HashSet::new(), true, &failing_executor(&fail_set))
            .expect("schedule should succeed");

        assert!(result.succeeded.contains(&job_id!("start")));
        assert!(result.succeeded.contains(&job_id!("A")));
        assert!(result.succeeded.contains(&job_id!("C")));
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].0, job_id!("B"));
        assert_eq!(result.skipped.len(), 0);
    }

    #[test]
    fn test_continue_on_failure_all_succeed() {
        let graph = graph! {
            "A" => [],
            "B" => ["A"],
            "C" => ["A"],
            "D" => ["B", "C"],
        };
        let result = run_wave_schedule(&graph, &HashSet::new(), true, &success_executor)
            .expect("schedule should succeed");

        assert_eq!(result.failed.len(), 0);
        assert_eq!(result.skipped.len(), 0);
        assert_eq!(result.succeeded.len(), 4);
    }

    #[test]
    fn test_continue_on_failure_complex_scenario() {
        let graph = graph! {
            "A" => [],
            "B" => ["A"],
            "C" => ["B"],
            "X" => [],
            "Y" => ["X"],
            "Z" => ["Y"],
            "P" => [],
            "Q" => ["P"],
            "R" => ["P"],
        };
        let fail_set: HashSet<JobId> = [job_id!("A"), job_id!("Q")].into_iter().collect();
        let result = run_wave_schedule(&graph, &HashSet::new(), true, &failing_executor(&fail_set))
            .expect("schedule should succeed");

        assert_eq!(result.failed.len(), 2);
        let failed_ids: HashSet<_> = result.failed.iter().map(|(id, _)| id.clone()).collect();
        assert!(failed_ids.contains(&job_id!("A")));
        assert!(failed_ids.contains(&job_id!("Q")));

        assert!(result.skipped.contains(&job_id!("B")));
        assert!(result.skipped.contains(&job_id!("C")));

        assert!(result.succeeded.contains(&job_id!("X")));
        assert!(result.succeeded.contains(&job_id!("Y")));
        assert!(result.succeeded.contains(&job_id!("Z")));
        assert!(result.succeeded.contains(&job_id!("P")));
        assert!(result.succeeded.contains(&job_id!("R")));
    }
}
