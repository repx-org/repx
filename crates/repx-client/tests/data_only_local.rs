use repx_client::client::Client;
use repx_core::config::{Config, SchedulerConfig, Target};
use std::collections::BTreeMap;

#[test]
fn test_data_only_local_target_initialization() {
    let temp_dir = tempfile::tempdir().expect("tempdir creation must succeed");
    let base_path = temp_dir.path().join("repx-data");

    let lab_path_str = std::env::var("REFERENCE_LAB_PATH").expect("REFERENCE_LAB_PATH must be set");
    let lab_path = std::path::PathBuf::from(lab_path_str);

    let mut targets = BTreeMap::new();
    targets.insert(
        "local".to_string(),
        Target {
            address: None,
            base_path: base_path.clone(),
            node_local_path: None,
            default_scheduler: None,
            default_execution_type: None,
            mount_host_paths: false,
            mount_paths: vec![],
            local: Some(SchedulerConfig {
                execution_types: vec![],
                local_concurrency: None,
            }),
            slurm: None,
        },
    );

    let config = Config {
        theme: None,
        submission_target: None,
        default_scheduler: None,
        logging: Default::default(),
        targets,
    };

    let _client = Client::new(config, lab_path.clone()).expect("Client initialization failed");

    assert!(base_path.join("repx").join("state").exists());
}

#[test]
fn test_missing_local_target_error() {
    let lab_path_str = std::env::var("REFERENCE_LAB_PATH").expect("REFERENCE_LAB_PATH must be set");
    let lab_path = std::path::PathBuf::from(lab_path_str);

    let config = Config {
        theme: None,
        submission_target: None,
        default_scheduler: None,
        logging: Default::default(),
        targets: BTreeMap::new(),
    };

    match Client::new(config, lab_path) {
        Ok(_) => panic!("Client initialization should have failed"),
        Err(e) => {
            let err_msg = e.to_string();
            assert!(err_msg.contains("A 'local' target must be defined"));
            assert!(err_msg.contains("data-only"));
        }
    }
}
