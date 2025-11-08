use repx_core::model::JobId;
use std::collections::{HashMap, HashSet};
use thiserror::Error;

#[derive(Error, Debug, PartialEq, Eq)]
enum SchedulerError {
    #[error("A cycle was detected in the job dependency graph. Remaining jobs: {0:?}")]
    CycleDetected(Vec<String>),
}

struct SchedulerTestHarness {
    jobs_to_submit: HashMap<JobId, Vec<JobId>>,
    submitted_jobs: HashSet<JobId>,
    submission_log: Vec<HashSet<JobId>>,
}

impl SchedulerTestHarness {
    fn new(graph: HashMap<JobId, Vec<JobId>>, pre_completed: HashSet<JobId>) -> Self {
        SchedulerTestHarness {
            jobs_to_submit: graph,
            submitted_jobs: pre_completed,
            submission_log: Vec::new(),
        }
    }

    fn run(&mut self) -> Result<(), SchedulerError> {
        let mut jobs_to_process = self.jobs_to_submit.clone();

        while !jobs_to_process.is_empty() {
            let current_wave: HashSet<JobId> = jobs_to_process
                .iter()
                .filter(|(_, deps)| {
                    deps.iter()
                        .all(|dep_id| self.submitted_jobs.contains(dep_id))
                })
                .map(|(job_id, _)| job_id.clone())
                .collect();

            if current_wave.is_empty() {
                let mut remaining: Vec<String> =
                    jobs_to_process.keys().map(|j| j.0.clone()).collect();
                remaining.sort();
                return Err(SchedulerError::CycleDetected(remaining));
            }

            for job_id in &current_wave {
                self.submitted_jobs.insert(job_id.clone());
                jobs_to_process.remove(job_id);
            }

            self.submission_log.push(current_wave);
        }

        Ok(())
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

macro_rules! waves {
    ( $( [ $( $job:expr ),* ] ),* $(,)? ) => {
        {
            let mut v = Vec::new();
            $(
                let mut s = ::std::collections::HashSet::new();
                $(
                    s.insert(job_id!($job));
                )*
                v.push(s);
            )*
            v
        }
    };
}

#[test]
fn test_simple_linear_chain() {
    let graph = graph! {
        "A" => [],
        "B" => ["A"],
        "C" => ["B"],
    };
    let mut harness = SchedulerTestHarness::new(graph, HashSet::new());
    harness.run().unwrap();

    let expected = waves! {
        ["A"],
        ["B"],
        ["C"],
    };
    assert_eq!(harness.submission_log, expected);
}

#[test]
fn test_simple_fan_out() {
    let graph = graph! {
        "A" => [],
        "B" => ["A"],
        "C" => ["A"],
    };
    let mut harness = SchedulerTestHarness::new(graph, HashSet::new());
    harness.run().unwrap();

    let expected = waves! {
        ["A"],
        ["B", "C"],
    };
    assert_eq!(harness.submission_log, expected);
}

#[test]
fn test_simple_fan_in() {
    let graph = graph! {
        "A" => [],
        "B" => [],
        "C" => ["A", "B"],
    };
    let mut harness = SchedulerTestHarness::new(graph, HashSet::new());
    harness.run().unwrap();

    let expected = waves! {
        ["A", "B"],
        ["C"],
    };
    assert_eq!(harness.submission_log, expected);
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
    let mut harness = SchedulerTestHarness::new(graph, HashSet::new());
    harness.run().unwrap();

    let expected = waves! {
        ["A"],
        ["B", "C"],
        ["D", "E"],
    };
    assert_eq!(harness.submission_log, expected);
}

#[test]
fn test_disconnected_graphs() {
    let graph = graph! {
        "A" => [],
        "B" => ["A"],
        "X" => [],
        "Y" => ["X"],
    };
    let mut harness = SchedulerTestHarness::new(graph, HashSet::new());
    harness.run().unwrap();

    let expected = waves! {
        ["A", "X"],
        ["B", "Y"],
    };
    assert_eq!(harness.submission_log, expected);
}

#[test]
fn test_graph_with_pre_completed_dependency() {
    let graph = graph! {
        "A" => [],
        "C" => ["A"],
        "D" => ["B", "C"],
        "E" => ["C"],
    };

    let pre_completed = waves! { ["B"] }[0].clone();
    let mut harness = SchedulerTestHarness::new(graph, pre_completed);
    harness.run().unwrap();

    let expected = waves! {
        ["A"],
        ["C"],
        ["D", "E"],
    };
    assert_eq!(harness.submission_log, expected);
}

#[test]
fn test_cycle_detection() {
    let graph = graph! {
        "A" => ["C"],
        "B" => ["A"],
        "C" => ["B"],
    };
    let mut harness = SchedulerTestHarness::new(graph, HashSet::new());
    let result = harness.run();

    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        SchedulerError::CycleDetected(vec!["A".to_string(), "B".to_string(), "C".to_string()])
    );
    assert!(harness.submission_log.is_empty());
}

#[test]
fn test_empty_input_graph() {
    let graph = graph! {};
    let mut harness = SchedulerTestHarness::new(graph, HashSet::new());
    harness.run().unwrap();

    let expected: Vec<HashSet<JobId>> = Vec::new();
    assert_eq!(harness.submission_log, expected);
}

/// Graph:
///       start
///      /     \
///   mid_a   mid_b
///      \     /
///        end
///
#[test]
fn test_diamond_dependency() {
    let graph = graph! {
        "start" => [],
        "mid_a" => ["start"],
        "mid_b" => ["start"],
        "end"   => ["mid_a", "mid_b"],
    };
    let mut harness = SchedulerTestHarness::new(graph, HashSet::new());
    harness.run().unwrap();

    let expected = waves! {
        ["start"],
        ["mid_a", "mid_b"],
        ["end"],
    };
    assert_eq!(harness.submission_log, expected);
}

/// Graph:
///   top_a ---------------+
///     |                  |
///     v                  v
/// independent_leaf   shared_mid
///                        |
///                        v
///   top_b----------->shared_leaf
///
#[test]
fn test_shared_sub_graph() {
    let graph = graph! {
        "top_a" => [],
        "top_b" => [],
        "independent_leaf" => ["top_a"],
        "shared_mid" => ["top_a", "top_b"],
        "shared_leaf" => ["shared_mid"],
    };
    let mut harness = SchedulerTestHarness::new(graph, HashSet::new());
    harness.run().unwrap();

    let expected = waves! {
        ["top_a", "top_b"],
        ["independent_leaf", "shared_mid"],
        ["shared_leaf"],
    };
    assert_eq!(harness.submission_log, expected);
}

/// Graph:
///   start_a          start_b          start_c
///    /   \            /   \            /   \
/// mid_a1 mid_a2    mid_b1 mid_b2    mid_c1 mid_c2
///    \   /            \   /            \   /
///    end_a            end_b            end_c
///
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
    let mut harness = SchedulerTestHarness::new(graph, HashSet::new());
    harness.run().unwrap();

    let expected = waves! {
        ["start_a", "start_b", "start_c"],
        ["mid_a1", "mid_a2", "mid_b1", "mid_b2", "mid_c1", "mid_c2"],
        ["end_a", "end_b", "end_c"],
    };
    assert_eq!(harness.submission_log, expected);
}

/// Graph:
///   start_a ────> mid_a1 ────> end_a
///      │             ▲           ▲
///      │             │           │
///      └───────────> mid_a2 ──────┘
///                        │
///                        ▼
///   start_b ────> mid_b1 ────> end_b
///
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

    let mut harness = SchedulerTestHarness::new(graph, HashSet::new());
    harness.run().unwrap();

    let expected = waves! {
        ["start_a", "start_b"],

        ["mid_a1", "mid_b1", "mid_a2"],

        ["end_a", "end_b"],
    };
    assert_eq!(harness.submission_log, expected);
}
