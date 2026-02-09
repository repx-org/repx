use repx_client::client::Client;
use repx_core::config::{Config, Target};

use std::collections::BTreeMap;
use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn create_dummy_image(
    work_dir: &std::path::Path,
    image_name: &str,
    layers: &[(&str, &str)],
) -> std::path::PathBuf {
    let image_dir = work_dir.join(format!("{}_content", image_name));
    fs::create_dir_all(&image_dir).unwrap();

    let mut manifest_layers = Vec::new();

    for (hash, content) in layers {
        let layer_dir = image_dir.join(hash);
        fs::create_dir_all(&layer_dir).unwrap();
        let layer_tar = layer_dir.join("layer.tar");
        fs::write(&layer_tar, content).unwrap();
        manifest_layers.push(format!("{}/layer.tar", hash));
    }

    let manifest_json = serde_json::to_string(&vec![serde_json::json!({
        "Layers": manifest_layers
    })])
    .unwrap();

    fs::write(image_dir.join("manifest.json"), manifest_json).unwrap();

    let image_tar = work_dir.join(format!("{}.tar", image_name));

    let mut tar_cmd = Command::new("tar");
    tar_cmd
        .arg("-cf")
        .arg(&image_tar)
        .arg("-C")
        .arg(&image_dir)
        .arg("manifest.json");

    for (hash, _) in layers {
        tar_cmd.arg(hash);
    }

    let status = tar_cmd.status().unwrap();
    assert!(status.success());

    image_tar
}

#[test]
fn test_local_target_smart_sync() {
    let temp_dir = TempDir::new().unwrap();
    let base_path = temp_dir.path().join("repx-data");
    let cache_root = temp_dir.path().join("local-cache");

    fs::create_dir_all(&base_path).unwrap();
    fs::create_dir_all(&cache_root).unwrap();

    let lab_path_str = std::env::var("REFERENCE_LAB_PATH").unwrap_or_else(|_| ".".to_string());
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
            local: Some(repx_core::config::SchedulerConfig {
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

    let client = Client::new(config, lab_path).expect("Client initialization failed");
    let target = client.get_target("local").expect("Local target not found");

    let image1_path = create_dummy_image(
        temp_dir.path(),
        "image1",
        &[("hashA", "contentA"), ("hashB", "contentB")],
    );

    target
        .sync_image_incrementally(&image1_path, "latest", &cache_root)
        .expect("Sync failed");

    let store_dir = cache_root.join("store");
    assert!(
        store_dir.join("hashA-layer.tar").exists(),
        "hashA-layer.tar should exist in store"
    );
    assert!(
        store_dir.join("hashB-layer.tar").exists(),
        "hashB-layer.tar should exist in store"
    );

    let images_dir = cache_root.join("images");
    let image1_cache = images_dir.join("image1");
    assert!(image1_cache.exists());

    let link_a = image1_cache.join("hashA/layer.tar");
    assert!(link_a.is_symlink());
    let target_path = fs::read_link(&link_a).unwrap();
    let expected_target = store_dir.join("hashA-layer.tar");
    assert_eq!(target_path, expected_target);

    let image2_path = create_dummy_image(
        temp_dir.path(),
        "image2",
        &[("hashA", "contentA"), ("hashC", "contentC")],
    );

    target
        .sync_image_incrementally(&image2_path, "v2", &cache_root)
        .expect("Sync failed");

    assert!(
        store_dir.join("hashC-layer.tar").exists(),
        "hashC-layer.tar should exist in store"
    );
    assert!(
        store_dir.join("hashA-layer.tar").exists(),
        "hashA-layer.tar should still exist"
    );

    let count = fs::read_dir(&store_dir)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .ok()
                .map(|e| e.file_name().to_string_lossy().ends_with("-layer.tar"))
                .unwrap_or(false)
        })
        .count();
    assert_eq!(count, 3, "Should have exactly 3 layers (A, B, C) in store");

    let image2_cache = images_dir.join("image2");
    assert!(image2_cache.exists());
    let link_a_in_image2 = image2_cache.join("hashA/layer.tar");
    assert!(link_a_in_image2.is_symlink());
    let target_path_2 = fs::read_link(&link_a_in_image2).unwrap();
    assert_eq!(
        target_path_2, expected_target,
        "image2 should share same layer as image1"
    );
}
