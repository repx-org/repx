#[cfg(test)]
mod integration {
    use crate::blueprint::*;
    use crate::cartesian::build_axes;
    use crate::expand::*;
    use serde_json::json;
    use std::collections::BTreeMap;

    fn empty_pipeline() -> PipelineTemplate {
        PipelineTemplate {
            source: "test".into(),
            stages: vec![],
        }
    }

    fn simple_stage(pname: &str, version: &str, script_drv: &str) -> StageTemplate {
        StageTemplate {
            pname: pname.into(),
            version: version.into(),
            stage_type: StageType::Simple,
            input_mappings: vec![],
            outputs: BTreeMap::new(),
            resources: None,
            executables: {
                let mut m = BTreeMap::new();
                m.insert(
                    "main".into(),
                    ExecutableTemplate {
                        inputs: vec![],
                        outputs: BTreeMap::new(),
                        resource_hints: None,
                        deps: vec![],
                    },
                );
                m
            },
            script_drv: Some(script_drv.into()),
            scatter_drv: None,
            gather_drv: None,
            step_drvs: None,
            step_deps: None,
        }
    }

    fn make_run(
        name: &str,
        axes: BTreeMap<String, Vec<serde_json::Value>>,
        stages: Vec<StageTemplate>,
    ) -> RunTemplate {
        RunTemplate {
            name: name.into(),
            hash_mode: HashMode::ParamsOnly,
            inter_run_dep_types: BTreeMap::new(),
            parameter_axes: axes,
            zip_groups: vec![],
            pipelines: vec![PipelineTemplate {
                source: "test-pipeline".into(),
                stages,
            }],
            image_path: None,
            image_contents: vec![],
        }
    }

    #[test]
    fn test_combination_count_simple() {
        let mut axes = BTreeMap::new();
        axes.insert("mode".into(), vec![json!("fast"), json!("slow")]);
        axes.insert("count".into(), vec![json!(1), json!(2), json!(3)]);
        let run = make_run("test", axes, vec![]);
        let (_ax, total) = build_axes(&run);
        assert_eq!(total, 6);
    }

    #[test]
    fn test_combination_count_zip() {
        let mut axes = BTreeMap::new();
        axes.insert("workload".into(), vec![json!("a"), json!("b"), json!("c")]);

        let mut run = make_run("test", axes, vec![]);
        run.zip_groups = vec![ZipGroup {
            members: vec!["vf_enable".into(), "label".into()],
            values: vec![
                {
                    let mut r = BTreeMap::new();
                    r.insert("vf_enable".into(), json!(0));
                    r.insert("label".into(), json!("baseline"));
                    r
                },
                {
                    let mut r = BTreeMap::new();
                    r.insert("vf_enable".into(), json!(1));
                    r.insert("label".into(), json!("vf"));
                    r
                },
            ],
        }];

        let (_ax, total) = build_axes(&run);
        assert_eq!(total, 6);
    }

    #[test]
    fn test_combination_count_multi_zip() {
        let axes = BTreeMap::new();
        let mut run = make_run("test", axes, vec![]);
        run.zip_groups = vec![
            ZipGroup {
                members: vec!["x".into(), "y".into()],
                values: vec![
                    {
                        let mut r = BTreeMap::new();
                        r.insert("x".into(), json!(1));
                        r.insert("y".into(), json!("a"));
                        r
                    },
                    {
                        let mut r = BTreeMap::new();
                        r.insert("x".into(), json!(2));
                        r.insert("y".into(), json!("b"));
                        r
                    },
                ],
            },
            ZipGroup {
                members: vec!["p".into(), "q".into()],
                values: vec![
                    {
                        let mut r = BTreeMap::new();
                        r.insert("p".into(), json!(10));
                        r.insert("q".into(), json!("X"));
                        r
                    },
                    {
                        let mut r = BTreeMap::new();
                        r.insert("p".into(), json!(20));
                        r.insert("q".into(), json!("Y"));
                        r
                    },
                    {
                        let mut r = BTreeMap::new();
                        r.insert("p".into(), json!(30));
                        r.insert("q".into(), json!("Z"));
                        r
                    },
                ],
            },
        ];

        let (_ax, total) = build_axes(&run);
        assert_eq!(total, 6);
    }

    #[test]
    fn test_combination_count_zip_only() {
        let axes = BTreeMap::new();
        let mut run = make_run("test", axes, vec![]);
        run.zip_groups = vec![ZipGroup {
            members: vec!["a".into(), "b".into()],
            values: vec![
                {
                    let mut r = BTreeMap::new();
                    r.insert("a".into(), json!(1));
                    r.insert("b".into(), json!("x"));
                    r
                },
                {
                    let mut r = BTreeMap::new();
                    r.insert("a".into(), json!(2));
                    r.insert("b".into(), json!("y"));
                    r
                },
                {
                    let mut r = BTreeMap::new();
                    r.insert("a".into(), json!(3));
                    r.insert("b".into(), json!("z"));
                    r
                },
            ],
        }];

        let (_ax, total) = build_axes(&run);
        assert_eq!(total, 3);
    }

    #[test]
    fn test_combination_count_no_zip() {
        let mut axes = BTreeMap::new();
        axes.insert("x".into(), vec![json!(1), json!(2)]);
        axes.insert("y".into(), vec![json!("a"), json!("b"), json!("c")]);
        let run = make_run("test", axes, vec![]);
        let (_ax, total) = build_axes(&run);
        assert_eq!(total, 6);
    }

    #[test]
    fn test_hash_stability_same_inputs() {
        let mut axes = BTreeMap::new();
        axes.insert("x".into(), vec![json!(1)]);
        let stage = simple_stage("stage-A", "1.0", "/nix/store/fake-drv");
        let run = make_run("test", axes, vec![stage]);

        let expanded1 = expand_run(&run, &BTreeMap::new());
        let expanded2 = expand_run(&run, &BTreeMap::new());

        assert_eq!(expanded1.jobs.len(), expanded2.jobs.len());
        for (a, b) in expanded1.jobs.iter().zip(expanded2.jobs.iter()) {
            assert_eq!(
                a.job_id, b.job_id,
                "Hash stability: same inputs must produce same IDs"
            );
        }
    }

    #[test]
    fn test_hash_different_params() {
        let stage = simple_stage("stage-A", "1.0", "/nix/store/fake-drv");

        let mut axes1 = BTreeMap::new();
        axes1.insert("x".into(), vec![json!(1)]);
        let run1 = make_run("test", axes1, vec![stage.clone()]);

        let mut axes2 = BTreeMap::new();
        axes2.insert("x".into(), vec![json!(2)]);
        let stage2 = simple_stage("stage-A", "1.0", "/nix/store/fake-drv");
        let run2 = make_run("test", axes2, vec![stage2]);

        let expanded1 = expand_run(&run1, &BTreeMap::new());
        let expanded2 = expand_run(&run2, &BTreeMap::new());

        assert_ne!(
            expanded1.jobs[0].job_id, expanded2.jobs[0].job_id,
            "Different params must produce different IDs"
        );
    }

    #[test]
    fn test_params_only_version_change_propagates() {
        let stage_a_v1 = simple_stage("stage-A", "1.0", "/nix/store/drv-a-v1");
        let stage_a_v2 = simple_stage("stage-A", "2.0", "/nix/store/drv-a-v1");

        let mut axes = BTreeMap::new();
        axes.insert("x".into(), vec![json!(1)]);

        let mut run1 = make_run("test", axes.clone(), vec![stage_a_v1]);
        run1.hash_mode = HashMode::ParamsOnly;

        let mut run2 = make_run("test", axes, vec![stage_a_v2]);
        run2.hash_mode = HashMode::ParamsOnly;

        let e1 = expand_run(&run1, &BTreeMap::new());
        let e2 = expand_run(&run2, &BTreeMap::new());

        assert_ne!(
            e1.jobs[0].job_id, e2.jobs[0].job_id,
            "params-only: version change must change hash"
        );
    }

    #[test]
    fn test_params_only_drv_change_no_effect() {
        let stage1 = simple_stage("stage-A", "1.0", "/nix/store/drv-v1");
        let stage2 = simple_stage("stage-A", "1.0", "/nix/store/drv-v2-different");

        let mut axes = BTreeMap::new();
        axes.insert("x".into(), vec![json!(1)]);

        let mut run1 = make_run("test", axes.clone(), vec![stage1]);
        run1.hash_mode = HashMode::ParamsOnly;

        let mut run2 = make_run("test", axes, vec![stage2]);
        run2.hash_mode = HashMode::ParamsOnly;

        let e1 = expand_run(&run1, &BTreeMap::new());
        let e2 = expand_run(&run2, &BTreeMap::new());

        assert_eq!(
            e1.jobs[0].job_id, e2.jobs[0].job_id,
            "params-only: drv change must NOT change hash"
        );
    }

    #[test]
    fn test_pure_drv_change_propagates() {
        let stage1 = simple_stage("stage-A", "1.0", "/nix/store/drv-v1");
        let stage2 = simple_stage("stage-A", "1.0", "/nix/store/drv-v2-different");

        let mut axes = BTreeMap::new();
        axes.insert("x".into(), vec![json!(1)]);

        let mut run1 = make_run("test", axes.clone(), vec![stage1]);
        run1.hash_mode = HashMode::Pure;

        let mut run2 = make_run("test", axes, vec![stage2]);
        run2.hash_mode = HashMode::Pure;

        let e1 = expand_run(&run1, &BTreeMap::new());
        let e2 = expand_run(&run2, &BTreeMap::new());

        assert_ne!(
            e1.jobs[0].job_id, e2.jobs[0].job_id,
            "pure: drv change must change hash"
        );
    }

    #[test]
    fn test_pure_vs_params_produce_different_hashes() {
        let stage = simple_stage("stage-A", "1.0", "/nix/store/fake-drv");

        let mut axes = BTreeMap::new();
        axes.insert("x".into(), vec![json!(1)]);

        let mut run_pure = make_run("test", axes.clone(), vec![stage.clone()]);
        run_pure.hash_mode = HashMode::Pure;

        let mut run_params = make_run("test", axes, vec![stage]);
        run_params.hash_mode = HashMode::ParamsOnly;

        let e_pure = expand_run(&run_pure, &BTreeMap::new());
        let e_params = expand_run(&run_params, &BTreeMap::new());

        assert_ne!(
            e_pure.jobs[0].job_id, e_params.jobs[0].job_id,
            "pure vs params-only must produce different hashes"
        );
    }

    #[test]
    fn test_dedup_identical_combos() {
        let stage = simple_stage("stage-A", "1.0", "/nix/store/fake-drv");

        let mut axes = BTreeMap::new();
        axes.insert("x".into(), vec![json!(1), json!(1)]);

        let run = make_run("test", axes, vec![stage]);
        let expanded = expand_run(&run, &BTreeMap::new());

        assert_eq!(expanded.jobs.len(), 2);
        assert_eq!(
            expanded.jobs[0].job_id, expanded.jobs[1].job_id,
            "Identical params must produce identical job IDs for dedup"
        );
    }

    #[test]
    fn test_pipeline_upstream_propagation() {
        let stage_a = simple_stage("stage-A", "1.0", "/nix/store/drv-a");
        let mut stage_b = simple_stage("stage-B", "1.0", "/nix/store/drv-b");
        stage_b.input_mappings = vec![InputMapping {
            mapping_type: Some("intra-pipeline".into()),
            job_id_template: Some("stage-A".into()),
            source_output: Some("result".into()),
            target_input: "a_result".into(),
            source_run: None,
            dependency_type: None,
            source_value: None,
            source: None,
            source_key: None,
            job_id: None,
        }];

        let mut axes = BTreeMap::new();
        axes.insert("x".into(), vec![json!(1)]);
        let run = make_run("test", axes, vec![stage_a, stage_b]);

        let expanded = expand_run(&run, &BTreeMap::new());
        assert_eq!(expanded.jobs.len(), 2);

        let id_a = &expanded.jobs[0].job_id;
        let id_b = &expanded.jobs[1].job_id;
        assert_ne!(id_a, id_b, "Different stages must have different IDs");
    }
}
