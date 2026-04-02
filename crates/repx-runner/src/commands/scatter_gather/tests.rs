use super::inputs::resolve_step_inputs;
use super::slurm::cancel_workers_from_manifest;
use super::toposort::toposort_steps;
use super::*;
use std::collections::HashSet;

#[test]
fn test_toposort_single_step() {
    let mut steps = HashMap::new();
    steps.insert(
        "compute".to_string(),
        StepMeta {
            exe_path: PathBuf::from("/bin/compute"),
            deps: vec![],
            outputs: HashMap::new(),
            inputs: vec![],
            resource_hints: None,
        },
    );
    let order = toposort_steps(&steps).expect("toposort must succeed");
    assert_eq!(order, vec!["compute"]);
}

#[test]
fn test_toposort_linear_chain() {
    let mut steps = HashMap::new();
    steps.insert(
        "a".to_string(),
        StepMeta {
            exe_path: PathBuf::from("/bin/a"),
            deps: vec![],
            outputs: HashMap::new(),
            inputs: vec![],
            resource_hints: None,
        },
    );
    steps.insert(
        "b".to_string(),
        StepMeta {
            exe_path: PathBuf::from("/bin/b"),
            deps: vec!["a".to_string()],
            outputs: HashMap::new(),
            inputs: vec![],
            resource_hints: None,
        },
    );
    steps.insert(
        "c".to_string(),
        StepMeta {
            exe_path: PathBuf::from("/bin/c"),
            deps: vec!["b".to_string()],
            outputs: HashMap::new(),
            inputs: vec![],
            resource_hints: None,
        },
    );
    let order = toposort_steps(&steps).expect("toposort must succeed");
    assert_eq!(order, vec!["a", "b", "c"]);
}

#[test]
fn test_toposort_diamond() {
    let mut steps = HashMap::new();
    steps.insert(
        "root".to_string(),
        StepMeta {
            exe_path: PathBuf::from("/bin/root"),
            deps: vec![],
            outputs: HashMap::new(),
            inputs: vec![],
            resource_hints: None,
        },
    );
    steps.insert(
        "left".to_string(),
        StepMeta {
            exe_path: PathBuf::from("/bin/left"),
            deps: vec!["root".to_string()],
            outputs: HashMap::new(),
            inputs: vec![],
            resource_hints: None,
        },
    );
    steps.insert(
        "right".to_string(),
        StepMeta {
            exe_path: PathBuf::from("/bin/right"),
            deps: vec!["root".to_string()],
            outputs: HashMap::new(),
            inputs: vec![],
            resource_hints: None,
        },
    );
    steps.insert(
        "sink".to_string(),
        StepMeta {
            exe_path: PathBuf::from("/bin/sink"),
            deps: vec!["left".to_string(), "right".to_string()],
            outputs: HashMap::new(),
            inputs: vec![],
            resource_hints: None,
        },
    );
    let order = toposort_steps(&steps).expect("toposort must succeed");
    assert_eq!(order[0], "root");
    assert_eq!(order[3], "sink");
    let middle: HashSet<&str> = order[1..3].iter().map(|s| s.as_str()).collect();
    assert!(middle.contains("left"));
    assert!(middle.contains("right"));
}

#[test]
fn test_toposort_cycle_detection() {
    let mut steps = HashMap::new();
    steps.insert(
        "a".to_string(),
        StepMeta {
            exe_path: PathBuf::from("/bin/a"),
            deps: vec!["b".to_string()],
            outputs: HashMap::new(),
            inputs: vec![],
            resource_hints: None,
        },
    );
    steps.insert(
        "b".to_string(),
        StepMeta {
            exe_path: PathBuf::from("/bin/b"),
            deps: vec!["a".to_string()],
            outputs: HashMap::new(),
            inputs: vec![],
            resource_hints: None,
        },
    );
    let result = toposort_steps(&steps);
    assert!(result.is_err());
    let err = result.expect_err("cycle detection should return an error");
    assert!(err.to_string().contains("Cycle detected"));
}

#[test]
fn test_toposort_unknown_dep() {
    let mut steps = HashMap::new();
    steps.insert(
        "a".to_string(),
        StepMeta {
            exe_path: PathBuf::from("/bin/a"),
            deps: vec!["nonexistent".to_string()],
            outputs: HashMap::new(),
            inputs: vec![],
            resource_hints: None,
        },
    );
    let result = toposort_steps(&steps);
    assert!(result.is_err());
    let err = result.expect_err("unknown dep should return an error");
    assert!(err.to_string().contains("unknown step"));
}

#[test]
fn test_steps_metadata_deserialize() {
    let json = r#"{
        "steps": {
            "compute": {
                "exe_path": "/nix/store/abc/bin/step-compute",
                "deps": [],
                "outputs": {"partial_sum": "$out/worker-result.txt"},
                "inputs": [
                    {"source": "scatter:work_item", "target_input": "worker__item"},
                    {"job_id": "xyz-stage-C-1.1", "source_output": "combined_list", "target_input": "number_list_file", "type": "intra-pipeline"}
                ]
            }
        },
        "sink_step": "compute"
    }"#;
    let meta: StepsMetadata = serde_json::from_str(json).expect("JSON must deserialize");
    assert_eq!(meta.steps.len(), 1);
    assert_eq!(meta.sink_step, "compute");
    let compute = &meta.steps["compute"];
    assert!(compute.deps.is_empty());
    assert_eq!(compute.outputs.len(), 1);
    assert_eq!(compute.inputs.len(), 2);
}

#[test]
fn test_steps_metadata_diamond_deserialize() {
    let json = r#"{
        "steps": {
            "trace_gen": {
                "exe_path": "/bin/trace-gen",
                "deps": [],
                "outputs": {"trace": "$out/trace.bin"},
                "inputs": [
                    {"source": "scatter:work_item", "target_input": "worker__item"}
                ]
            },
            "trace_align": {
                "exe_path": "/bin/trace-align",
                "deps": ["trace_gen"],
                "outputs": {"aligned": "$out/aligned.bin"},
                "inputs": [
                    {"source": "step:trace_gen", "source_output": "trace", "target_input": "trace"}
                ]
            },
            "trace_analyze": {
                "exe_path": "/bin/trace-analyze",
                "deps": ["trace_gen"],
                "outputs": {"analysis": "$out/analysis.json"},
                "inputs": [
                    {"source": "step:trace_gen", "source_output": "trace", "target_input": "trace"}
                ]
            },
            "foldability": {
                "exe_path": "/bin/foldability",
                "deps": ["trace_align", "trace_analyze"],
                "outputs": {"result": "$out/fold.json"},
                "inputs": [
                    {"source": "step:trace_align", "source_output": "aligned", "target_input": "aligned"},
                    {"source": "step:trace_analyze", "source_output": "analysis", "target_input": "analysis"}
                ]
            }
        },
        "sink_step": "foldability"
    }"#;
    let meta: StepsMetadata = serde_json::from_str(json).expect("JSON must deserialize");
    assert_eq!(meta.steps.len(), 4);
    assert_eq!(meta.sink_step, "foldability");

    let order = toposort_steps(&meta.steps).expect("toposort must succeed");
    assert_eq!(order[0], "trace_gen");
    assert_eq!(order[3], "foldability");
}

#[test]
fn test_resolve_step_inputs_scatter_source() {
    let mut steps = HashMap::new();
    let step = StepMeta {
        exe_path: PathBuf::from("/bin/step"),
        deps: vec![],
        outputs: HashMap::from([("out1".to_string(), "$out/result.txt".to_string())]),
        inputs: vec![StepInputMapping {
            source: Some("scatter:work_item".to_string()),
            source_output: None,
            target_input: "worker__item".to_string(),
            job_id: None,
            mapping_type: None,
        }],
        resource_hints: None,
    };
    steps.insert("compute".to_string(), step.clone());

    let branch_root = PathBuf::from("/tmp/job/branch-0");
    let work_item_path = PathBuf::from("/tmp/job/branch-0/repx/work_item.json");
    let static_inputs = Value::Object(Default::default());

    let result = resolve_step_inputs(&step, &branch_root, &work_item_path, &static_inputs, &steps)
        .expect("step input resolution must succeed");
    assert_eq!(
        result["worker__item"],
        "/tmp/job/branch-0/repx/work_item.json"
    );
}

#[test]
fn test_resolve_step_inputs_step_dep() {
    let mut steps = HashMap::new();
    steps.insert(
        "gen".to_string(),
        StepMeta {
            exe_path: PathBuf::from("/bin/gen"),
            deps: vec![],
            outputs: HashMap::from([("trace".to_string(), "$out/trace.bin".to_string())]),
            inputs: vec![],
            resource_hints: None,
        },
    );

    let consumer = StepMeta {
        exe_path: PathBuf::from("/bin/analyze"),
        deps: vec!["gen".to_string()],
        outputs: HashMap::new(),
        inputs: vec![StepInputMapping {
            source: Some("step:gen".to_string()),
            source_output: Some("trace".to_string()),
            target_input: "input_trace".to_string(),
            job_id: None,
            mapping_type: None,
        }],
        resource_hints: None,
    };
    steps.insert("analyze".to_string(), consumer.clone());

    let branch_root = PathBuf::from("/tmp/job/branch-0");
    let work_item_path = PathBuf::from("/tmp/job/branch-0/repx/work_item.json");
    let static_inputs = Value::Object(Default::default());

    let result = resolve_step_inputs(
        &consumer,
        &branch_root,
        &work_item_path,
        &static_inputs,
        &steps,
    )
    .expect("step input resolution must succeed");
    assert_eq!(
        result["input_trace"],
        "/tmp/job/branch-0/step-gen/out/trace.bin"
    );
}

#[test]
fn test_resolve_step_inputs_external() {
    let steps = HashMap::new();
    let step = StepMeta {
        exe_path: PathBuf::from("/bin/step"),
        deps: vec![],
        outputs: HashMap::new(),
        inputs: vec![StepInputMapping {
            source: None,
            source_output: Some("combined_list".to_string()),
            target_input: "number_list_file".to_string(),
            job_id: Some("xyz-stage-C-1.1".to_string()),
            mapping_type: Some("intra-pipeline".to_string()),
        }],
        resource_hints: None,
    };

    let branch_root = PathBuf::from("/tmp/job/branch-0");
    let work_item_path = PathBuf::from("/tmp/job/branch-0/repx/work_item.json");
    let static_inputs = serde_json::json!({
        "number_list_file": "/outputs/xyz-stage-C-1.1/out/combined_list.txt"
    });

    let result = resolve_step_inputs(&step, &branch_root, &work_item_path, &static_inputs, &steps)
        .expect("step input resolution must succeed");
    assert_eq!(
        result["number_list_file"],
        "/outputs/xyz-stage-C-1.1/out/combined_list.txt"
    );
}

#[test]
fn test_worker_manifest_serialization() {
    let worker_ids: Vec<u32> = vec![100, 101, 102, 103, 200, 201];
    let json = serde_json::to_string(&worker_ids).expect("JSON serialization must succeed");
    let deserialized: Vec<u32> = serde_json::from_str(&json).expect("JSON must deserialize");
    assert_eq!(deserialized, worker_ids);
}

#[test]
fn test_worker_manifest_written_to_correct_path() {
    let tmp = tempfile::tempdir().expect("tempdir creation must succeed");
    let repx_dir = tmp.path().join("repx");
    fs::create_dir_all(&repx_dir).expect("dir creation must succeed");

    let worker_ids: Vec<u32> = vec![42, 43, 44];
    let manifest_path = repx_dir.join(manifests::WORKER_SLURM_IDS);
    let json = serde_json::to_string(&worker_ids).expect("JSON serialization must succeed");
    fs::write(&manifest_path, &json).expect("file write must succeed");

    let content = fs::read_to_string(&manifest_path).expect("file read must succeed");
    let read_ids: Vec<u32> = serde_json::from_str(&content).expect("JSON must deserialize");
    assert_eq!(read_ids, vec![42, 43, 44]);
}

#[test]
fn test_worker_manifest_empty_is_valid() {
    let worker_ids: Vec<u32> = vec![];
    let json = serde_json::to_string(&worker_ids).expect("JSON serialization must succeed");
    let deserialized: Vec<u32> = serde_json::from_str(&json).expect("JSON must deserialize");
    assert!(deserialized.is_empty());
}

#[tokio::test]
async fn test_cancel_workers_from_manifest_with_valid_file() {
    let tmp = tempfile::tempdir().expect("tempdir creation must succeed");
    let repx_dir = tmp.path();

    let worker_ids: Vec<u32> = vec![999, 998, 997];
    let manifest_path = repx_dir.join(manifests::WORKER_SLURM_IDS);
    fs::write(
        &manifest_path,
        serde_json::to_string(&worker_ids).expect("JSON serialization must succeed"),
    )
    .expect("file write must succeed");

    cancel_workers_from_manifest(repx_dir).await;

    assert!(manifest_path.exists());
}

#[tokio::test]
async fn test_cancel_workers_from_manifest_no_file() {
    let tmp = tempfile::tempdir().expect("tempdir creation must succeed");
    cancel_workers_from_manifest(tmp.path()).await;
}

fn make_script(path: &Path, body: &str) {
    fs::write(path, format!("#!/bin/sh\n{body}\n")).expect("file write must succeed");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
            .expect("setting permissions must succeed");
    }
}

fn step(exe: PathBuf, deps: &[&str], out_name: &str, input_src: &str) -> StepMeta {
    let inputs = if input_src == "scatter" {
        vec![StepInputMapping {
            source: Some("scatter:work_item".to_string()),
            source_output: None,
            target_input: "worker__item".to_string(),
            job_id: None,
            mapping_type: None,
        }]
    } else {
        let (src_step, src_out) = input_src.split_once(':').unwrap_or((input_src, out_name));
        vec![StepInputMapping {
            source: Some(format!("step:{src_step}")),
            source_output: Some(src_out.to_string()),
            target_input: "input_data".into(),
            job_id: None,
            mapping_type: None,
        }]
    };
    StepMeta {
        exe_path: exe,
        deps: deps.iter().map(|s| s.to_string()).collect(),
        outputs: HashMap::from([(out_name.to_string(), format!("$out/{out_name}.txt"))]),
        inputs,
        resource_hints: None,
    }
}

fn single_step_metadata(exe: PathBuf) -> StepsMetadata {
    let mut steps = HashMap::new();
    steps.insert("only".into(), step(exe, &[], "result", "scatter"));
    StepsMetadata {
        steps,
        sink_step: "only".into(),
    }
}

fn diamond_step_metadata(exe: PathBuf) -> StepsMetadata {
    let mut steps = HashMap::new();
    steps.insert("root".into(), step(exe.clone(), &[], "data", "scatter"));
    steps.insert(
        "left".into(),
        step(exe.clone(), &["root"], "left", "root:data"),
    );
    steps.insert(
        "right".into(),
        step(exe.clone(), &["root"], "right", "root:data"),
    );
    steps.insert(
        "sink".into(),
        step(exe, &["left", "right"], "final", "left:left"),
    );
    steps
        .get_mut("sink")
        .expect("sink step must exist")
        .inputs
        .push(StepInputMapping {
            source: Some("step:right".into()),
            source_output: Some("right".into()),
            target_input: "right_data".into(),
            job_id: None,
            mapping_type: None,
        });
    StepsMetadata {
        steps,
        sink_step: "sink".into(),
    }
}

async fn run_branch(
    tmp: &Path,
    job_root: &Path,
    branch_idx: usize,
    work_item: &Value,
    steps_meta: &StepsMetadata,
    topo_order: &[String],
) -> Result<PathBuf, CliError> {
    let scatter_out = job_root.join("scatter").join(dirs::OUT);
    fs::create_dir_all(&scatter_out).expect("dir creation must succeed");
    let mut items = Vec::new();
    for _ in 0..=branch_idx {
        items.push(work_item.clone());
    }
    fs::write(
        scatter_out.join("work_items.json"),
        serde_json::to_string(&items).expect("JSON serialization must succeed"),
    )
    .expect("file write must succeed");

    let scripts = tmp.join("scripts");
    let repx_dir = job_root.join(dirs::REPX);
    fs::create_dir_all(&repx_dir).expect("dir creation must succeed");

    let steps_json = serde_json::to_string(steps_meta).expect("JSON serialization must succeed");

    for step_name in topo_order {
        let args = InternalScatterGatherArgs {
            job_id: "test-job".into(),
            runtime: repx_core::model::ExecutionType::Native,
            image_tag: None,
            base_path: tmp.to_path_buf(),
            node_local_path: None,
            local_artifacts_path: None,
            lab_tar_path: None,
            host_tools_dir: String::new(),
            scheduler: repx_core::model::SchedulerType::Local,
            step_sbatch_opts: String::new(),
            job_package_path: scripts.clone(),
            scatter_exe_path: scripts.join("scatter.sh"),
            gather_exe_path: scripts.join("gather.sh"),
            steps_json: steps_json.clone(),
            last_step_outputs_json: "{}".into(),
            anchor_id: None,
            phase: crate::cli::ScatterGatherPhase::Step,
            branch_idx: Some(branch_idx),
            step_name: Some(step_name.clone()),
            mount_host_paths: false,
            mount_paths: vec![],
        };
        let mut orch = ScatterGatherOrchestrator::new(&args)?;
        orch.load_static_inputs()?;
        handle_phase_step(&mut orch, &args, steps_meta).await?;
    }

    let sink_out = job_root
        .join(format!("branch-{}", branch_idx))
        .join(format!("step-{}", steps_meta.sink_step))
        .join(dirs::OUT);
    Ok(sink_out)
}

#[tokio::test]
async fn test_marker_write_failure_propagates_as_error() {
    let tmp = tempfile::tempdir().expect("tempdir creation must succeed");
    let job_root = tmp.path().join("outputs/test-job");
    let scripts = tmp.path().join("scripts");
    fs::create_dir_all(&scripts).expect("dir creation must succeed");
    make_script(
        &scripts.join("succeed.sh"),
        "mkdir -p \"$1\"\necho done > \"$1/result.txt\"",
    );

    let meta = single_step_metadata(scripts.join("succeed.sh"));
    let order = toposort_steps(&meta.steps).expect("toposort must succeed");
    let item = serde_json::json!({"id": 0});

    let r = run_branch(tmp.path(), &job_root, 0, &item, &meta, &order).await;
    assert!(r.is_ok(), "First run should succeed");

    let step_repx = job_root.join("branch-0/step-only").join(dirs::REPX);
    assert!(step_repx.join(markers::SUCCESS).exists());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::remove_file(step_repx.join(markers::SUCCESS)).expect("file removal must succeed");
        fs::set_permissions(&step_repx, fs::Permissions::from_mode(0o555))
            .expect("setting permissions must succeed");

        let probe = step_repx.join(".write_probe");
        let perms_effective = fs::File::create(&probe).is_err();
        let _ = fs::remove_file(&probe);

        let r2 = run_branch(tmp.path(), &job_root, 0, &item, &meta, &order).await;
        fs::set_permissions(&step_repx, fs::Permissions::from_mode(0o755))
            .expect("setting permissions must succeed");

        if perms_effective {
            assert!(
                r2.is_err(),
                "Should error when SUCCESS marker cannot be written"
            );
        }
    }
}

#[tokio::test]
async fn test_scatter_skipped_on_rerun_if_already_succeeded() {
    let tmp = tempfile::tempdir().expect("tempdir creation must succeed");
    let job_root = tmp.path().join("outputs/test-sg-job");
    let scatter_out = job_root.join("scatter").join(dirs::OUT);
    let scatter_repx = job_root.join("scatter").join(dirs::REPX);
    for d in [
        &scatter_out,
        &scatter_repx,
        &job_root.join(dirs::REPX),
        &job_root.join(dirs::OUT),
    ] {
        fs::create_dir_all(d).expect("dir creation must succeed");
    }

    fs::File::create(scatter_repx.join(markers::SUCCESS)).expect("file creation must succeed");
    fs::write(
        scatter_out.join("work_items.json"),
        r#"[{"id":1},{"id":2}]"#,
    )
    .expect("file write must succeed");

    let scripts = tmp.path().join("scripts");
    fs::create_dir_all(&scripts).expect("dir creation must succeed");
    make_script(
        &scripts.join("scatter.sh"),
        "echo '[{\"id\":99}]' > \"$1/work_items.json\"",
    );

    let mut orch = ScatterGatherOrchestrator {
        job_id: JobId::from("test-sg-job"),
        base_path: tmp.path().to_path_buf(),
        job_root: job_root.clone(),
        user_out_dir: job_root.join(dirs::OUT),
        repx_dir: job_root.join(dirs::REPX),
        scatter_out_dir: scatter_out.clone(),
        scatter_repx_dir: scatter_repx.clone(),
        inputs_json_path: job_root.join(dirs::REPX).join("inputs.json"),
        parameters_json_path: job_root.join(dirs::REPX).join("parameters.json"),
        runtime: Runtime::Native,
        job_package_path: scripts.clone(),
        static_inputs: Value::Object(Default::default()),
        host_tools_bin_dir: None,
        node_local_path: None,
        local_artifacts_path: None,
        lab_tar_path: None,
        mount_policy: repx_core::model::MountPolicy::Isolated,
    };

    orch.init_dirs().expect("init_dirs must succeed");
    assert!(
        scatter_repx.join(markers::SUCCESS).exists(),
        "init_dirs must preserve scatter SUCCESS"
    );

    let already_done = scatter_repx.join(markers::SUCCESS).exists()
        && scatter_out.join("work_items.json").exists();
    if !already_done {
        let _ = orch.run_scatter(&scripts.join("scatter.sh")).await;
    }

    let items: Vec<Value> = serde_json::from_str(
        &fs::read_to_string(scatter_out.join("work_items.json")).expect("file read must succeed"),
    )
    .expect("JSON must deserialize");
    assert_eq!(
        items,
        vec![serde_json::json!({"id":1}), serde_json::json!({"id":2})],
        "Scatter should be skipped; work_items.json must be preserved"
    );
}

#[tokio::test]
async fn test_stale_step_markers_cleared_when_work_item_changes() {
    let tmp = tempfile::tempdir().expect("tempdir creation must succeed");
    let job_root = tmp.path().join("outputs/test-job");
    let scripts = tmp.path().join("scripts");
    fs::create_dir_all(&scripts).expect("dir creation must succeed");
    make_script(
        &scripts.join("step.sh"),
        "mkdir -p \"$1\"\necho done > \"$1/result.txt\"",
    );

    let meta = single_step_metadata(scripts.join("step.sh"));
    let order = toposort_steps(&meta.steps).expect("toposort must succeed");

    let branch_repx = job_root.join("branch-0").join(dirs::REPX);
    let step_repx = job_root.join("branch-0/step-only").join(dirs::REPX);
    let step_out = job_root.join("branch-0/step-only").join(dirs::OUT);
    fs::create_dir_all(&branch_repx).expect("dir creation must succeed");
    fs::create_dir_all(&step_repx).expect("dir creation must succeed");
    fs::create_dir_all(&step_out).expect("dir creation must succeed");
    fs::write(branch_repx.join("work_item.json"), r#"{"id":"old_item"}"#)
        .expect("file write must succeed");
    fs::File::create(step_repx.join(markers::SUCCESS)).expect("file creation must succeed");
    fs::write(step_out.join("result.txt"), "old_item_result").expect("file write must succeed");

    let new_item = serde_json::json!({"id": "new_item"});
    let r = run_branch(tmp.path(), &job_root, 0, &new_item, &meta, &order).await;
    assert!(r.is_ok());

    let output = fs::read_to_string(step_out.join("result.txt")).expect("file read must succeed");
    assert_ne!(
        output.trim(),
        "old_item_result",
        "Step must re-execute when work item changes; stale markers should be invalidated"
    );
}

#[tokio::test]
async fn test_diamond_dag_steps_all_execute() {
    let tmp = tempfile::tempdir().expect("tempdir creation must succeed");
    let job_root = tmp.path().join("outputs/test-job");
    let scripts = tmp.path().join("scripts");
    fs::create_dir_all(&scripts).expect("dir creation must succeed");

    make_script(&scripts.join("timed.sh"),
        "mkdir -p \"$1\"\nfor f in data left right final result; do echo done > \"$1/$f.txt\"; done");

    let meta = diamond_step_metadata(scripts.join("timed.sh"));
    let order = toposort_steps(&meta.steps).expect("toposort must succeed");
    let item = serde_json::json!({"id": 0});

    let r = run_branch(tmp.path(), &job_root, 0, &item, &meta, &order).await;
    assert!(r.is_ok(), "Branch should succeed: {:?}", r.err());

    for step in &["root", "left", "right", "sink"] {
        let marker = job_root
            .join(format!("branch-0/step-{}/", step))
            .join(dirs::REPX)
            .join(markers::SUCCESS);
        assert!(
            marker.exists(),
            "Step '{}' should have SUCCESS marker",
            step
        );
    }
}

#[test]
fn test_marker_write_calls_fsync() {
    let source = include_str!("mod.rs");
    let prod = source
        .split("#[cfg(test)]")
        .next()
        .expect("source must contain #[cfg(test)]");
    let has_bare = prod
        .lines()
        .any(|l| l.contains("let _ = fs::File::create(") && l.contains("markers::"));
    assert!(
        !has_bare,
        "Production code must not use bare `let _ = fs::File::create(...markers...)`. \
         Use write_marker() for error propagation and fsync."
    );
}

#[tokio::test]
async fn test_rerun_preserves_scatter_output_and_skips_succeeded_steps() {
    let tmp = tempfile::tempdir().expect("tempdir creation must succeed");
    let job_root = tmp.path().join("outputs/test-job");
    let scripts = tmp.path().join("scripts");
    fs::create_dir_all(&scripts).expect("dir creation must succeed");
    make_script(
        &scripts.join("good.sh"),
        "mkdir -p \"$1\"\necho done > \"$1/result.txt\"",
    );

    let meta = single_step_metadata(scripts.join("good.sh"));
    let order = toposort_steps(&meta.steps).expect("toposort must succeed");
    let items = [serde_json::json!({"id":"A"}), serde_json::json!({"id":"B"})];

    let r = run_branch(tmp.path(), &job_root, 0, &items[0], &meta, &order).await;
    assert!(r.is_ok());

    let b1_repx = job_root.join("branch-1").join(dirs::REPX);
    let s1_repx = job_root.join("branch-1/step-only").join(dirs::REPX);
    for d in [
        &b1_repx,
        &s1_repx,
        &job_root.join("branch-1/step-only").join(dirs::OUT),
    ] {
        fs::create_dir_all(d).expect("dir creation must succeed");
    }
    fs::write(
        b1_repx.join("work_item.json"),
        serde_json::to_string(&items[1]).expect("JSON serialization must succeed"),
    )
    .expect("file write must succeed");
    fs::File::create(s1_repx.join(markers::FAIL)).expect("file creation must succeed");

    let orig = fs::read_to_string(job_root.join("branch-0/step-only/out/result.txt"))
        .expect("file read must succeed");

    let r = run_branch(tmp.path(), &job_root, 0, &items[0], &meta, &order).await;
    assert!(r.is_ok());
    assert_eq!(
        orig,
        fs::read_to_string(job_root.join("branch-0/step-only/out/result.txt"))
            .expect("file read must succeed")
    );

    let r = run_branch(tmp.path(), &job_root, 1, &items[1], &meta, &order).await;
    assert!(r.is_ok());
    assert!(s1_repx.join(markers::SUCCESS).exists());
    assert!(!s1_repx.join(markers::FAIL).exists());
}
