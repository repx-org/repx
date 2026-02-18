use repx_core::model::JobId;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
enum JobResult {
    Success,
    Failed(String),
}

#[derive(Debug, PartialEq, Eq)]
struct BatchResult {
    succeeded: Vec<JobId>,
    failed: Vec<(JobId, String)>,
    skipped: Vec<JobId>,
}

struct ContinueOnFailureHarness {
    jobs_to_submit: HashMap<JobId, Vec<JobId>>,
    job_outcomes: HashMap<JobId, JobResult>,
    pre_completed: HashSet<JobId>,
    continue_on_failure: bool,
}

impl ContinueOnFailureHarness {
    fn new(
        graph: HashMap<JobId, Vec<JobId>>,
        outcomes: HashMap<JobId, JobResult>,
        pre_completed: HashSet<JobId>,
        continue_on_failure: bool,
    ) -> Self {
        Self {
            jobs_to_submit: graph,
            job_outcomes: outcomes,
            pre_completed,
            continue_on_failure,
        }
    }

    fn run(&self) -> Result<BatchResult, String> {
        let mut completed: HashSet<JobId> = self.pre_completed.clone();
        let mut failed: Vec<(JobId, String)> = vec![];
        let mut failed_ids: HashSet<JobId> = HashSet::new();
        let mut jobs_left: HashSet<JobId> = self.jobs_to_submit.keys().cloned().collect();

        for id in &self.pre_completed {
            jobs_left.remove(id);
        }

        loop {
            let ready: Vec<JobId> = jobs_left
                .iter()
                .filter(|job_id| {
                    let deps = self.jobs_to_submit.get(*job_id).unwrap();
                    let deps_met = deps.iter().all(|dep| completed.contains(dep));
                    let no_failed_deps = deps.iter().all(|dep| !failed_ids.contains(dep));
                    deps_met && no_failed_deps
                })
                .cloned()
                .collect();

            if ready.is_empty() {
                break;
            }

            for job_id in ready {
                jobs_left.remove(&job_id);

                let outcome = self
                    .job_outcomes
                    .get(&job_id)
                    .cloned()
                    .unwrap_or(JobResult::Success);

                match outcome {
                    JobResult::Success => {
                        completed.insert(job_id);
                    }
                    JobResult::Failed(msg) => {
                        if self.continue_on_failure {
                            failed_ids.insert(job_id.clone());
                            failed.push((job_id, msg));
                        } else {
                            return Err(format!("Job failed: {}", msg));
                        }
                    }
                }
            }
        }

        let skipped: Vec<JobId> = jobs_left.into_iter().collect();

        let succeeded: Vec<JobId> = completed.difference(&self.pre_completed).cloned().collect();

        Ok(BatchResult {
            succeeded,
            failed,
            skipped,
        })
    }
}

macro_rules! job_id {
    ($name:expr) => {
        JobId($name.to_string())
    };
}

macro_rules! graph {
    ( $( $job:expr => [ $( $dep:expr ),* ] ),* $(,)? ) => {
        ::std::collections::HashMap::from([
            $(
                (job_id!($job), vec![$(job_id!($dep)),*]),
            )*
        ])
    };
}

macro_rules! outcomes {
    ( $( $job:expr => $outcome:expr ),* $(,)? ) => {
        ::std::collections::HashMap::from([
            $(
                (job_id!($job), $outcome),
            )*
        ])
    };
}

#[test]
fn test_fail_fast_stops_on_first_failure() {
    let graph = graph! {
        "A" => [],
        "B" => [],
        "C" => ["A", "B"],
    };
    let outcomes = outcomes! {
        "A" => JobResult::Failed("A failed".to_string()),
    };

    let harness = ContinueOnFailureHarness::new(graph, outcomes, HashSet::new(), false);
    let result = harness.run();

    assert!(result.is_err());
    assert!(result.unwrap_err().contains("A failed"));
}

#[test]
fn test_fail_fast_all_succeed() {
    let graph = graph! {
        "A" => [],
        "B" => ["A"],
        "C" => ["B"],
    };

    let harness = ContinueOnFailureHarness::new(graph, HashMap::new(), HashSet::new(), false);
    let result = harness.run().unwrap();

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
    let outcomes = outcomes! {
        "A" => JobResult::Failed("A failed".to_string()),
    };

    let harness = ContinueOnFailureHarness::new(graph, outcomes, HashSet::new(), true);
    let result = harness.run().unwrap();

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
    let outcomes = outcomes! {
        "A" => JobResult::Failed("A failed".to_string()),
        "B" => JobResult::Failed("B failed".to_string()),
    };

    let harness = ContinueOnFailureHarness::new(graph, outcomes, HashSet::new(), true);
    let result = harness.run().unwrap();

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
    let outcomes = outcomes! {
        "A" => JobResult::Failed("A failed".to_string()),
    };

    let harness = ContinueOnFailureHarness::new(graph, outcomes, HashSet::new(), true);
    let result = harness.run().unwrap();

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
    let outcomes = outcomes! {
        "mid_a" => JobResult::Failed("mid_a failed".to_string()),
    };

    let harness = ContinueOnFailureHarness::new(graph, outcomes, HashSet::new(), true);
    let result = harness.run().unwrap();

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
    let outcomes = outcomes! {
        "B" => JobResult::Failed("B failed".to_string()),
    };

    let harness = ContinueOnFailureHarness::new(graph, outcomes, HashSet::new(), true);
    let result = harness.run().unwrap();

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

    let harness = ContinueOnFailureHarness::new(graph, HashMap::new(), HashSet::new(), true);
    let result = harness.run().unwrap();

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
    let outcomes = outcomes! {
        "A" => JobResult::Failed("A failed".to_string()),
        "Q" => JobResult::Failed("Q failed".to_string()),
    };

    let harness = ContinueOnFailureHarness::new(graph, outcomes, HashSet::new(), true);
    let result = harness.run().unwrap();

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
