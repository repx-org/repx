use crate::cli::{GcArgs, InternalGcArgs};
use crate::commands::AppContext;
use repx_client::error::ClientError;
use repx_core::{error::AppError, lab, log_info, log_warn};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

pub fn handle_gc(
    args: GcArgs,
    context: &AppContext<'_>,
    _config: &repx_core::config::Config,
) -> Result<(), AppError> {
    let target_name = args.target.as_deref().unwrap_or(context.submission_target);
    log_info!("Garbage collecting target '{}'...", target_name);

    let target = context
        .client
        .get_target(target_name)
        .ok_or_else(|| AppError::TargetNotFound(target_name.to_string()))?;

    if let Err(e) = target.deploy_repx_binary() {
        log_warn!(
            "Failed to verify/deploy repx binary: {}. Trying to run GC anyway.",
            e
        );
    }

    let output = target.garbage_collect().map_err(|e| match e {
        ClientError::Core(ae) => ae,
        _ => AppError::ExecutionFailed {
            message: "GC failed".into(),
            log_path: None,
            log_summary: e.to_string(),
        },
    })?;

    println!("{}", output);

    Ok(())
}

pub fn handle_internal_gc(args: InternalGcArgs) -> Result<(), AppError> {
    let base_path = args.base_path;
    let gcroots_dir = base_path.join("gcroots");
    let artifacts_dir = base_path.join("artifacts");
    let outputs_dir = base_path.join("outputs");

    if !gcroots_dir.exists() {
        log_info!(
            "No gcroots directory found at {}. Nothing to GC.",
            gcroots_dir.display()
        );
        return Ok(());
    }

    log_info!("Scanning GC roots in {}...", gcroots_dir.display());

    let mut live_artifacts = HashSet::new();
    let mut live_jobs = HashSet::new();

    let process_link = |path: PathBuf,
                        live_arts: &mut HashSet<PathBuf>,
                        live_js: &mut HashSet<String>|
     -> Result<(), AppError> {
        if let Ok(target) = fs::read_link(&path) {
            let abs_target = if target.is_absolute() {
                target
            } else {
                path.parent().unwrap().join(target)
            };

            if let Ok(canonical) = fs::canonicalize(&abs_target) {
                if canonical.starts_with(&artifacts_dir) {
                    if let Ok(rel) = canonical.strip_prefix(&artifacts_dir) {
                        live_arts.insert(rel.to_path_buf());
                    }
                    let lab_root = canonical.clone();

                    if let Ok(lab) = lab::load_from_path(&lab_root) {
                        for job_id in lab.jobs.keys() {
                            live_js.insert(job_id.0.clone());
                        }
                        for ref_file in &lab.referenced_files {
                            live_arts.insert(ref_file.clone());

                            if let Some(std::path::Component::Normal(c)) =
                                ref_file.components().next()
                            {
                                live_arts.insert(PathBuf::from(c));
                            }
                        }
                    } else {
                        log_warn!(
                            "Could not load lab metadata from artifact '{}'. Outputs for this lab might be collected.",
                            canonical.display()
                        );
                    }
                }
            }
        }
        Ok(())
    };

    let pinned_dir = gcroots_dir.join("pinned");
    if pinned_dir.exists() {
        for entry in fs::read_dir(&pinned_dir)? {
            let entry = entry?;
            process_link(entry.path(), &mut live_artifacts, &mut live_jobs)?;
        }
    }

    let auto_dir = gcroots_dir.join("auto");
    if auto_dir.exists() {
        for project_entry in fs::read_dir(&auto_dir)? {
            let project_entry = project_entry?;
            if project_entry.path().is_dir() {
                for link_entry in fs::read_dir(project_entry.path())? {
                    let link_entry = link_entry?;
                    process_link(link_entry.path(), &mut live_artifacts, &mut live_jobs)?;
                }
            }
        }
    }

    log_info!(
        "Found {} live artifact paths and {} live jobs.",
        live_artifacts.len(),
        live_jobs.len()
    );

    if artifacts_dir.exists() {
        let collection_dirs = [
            "host-tools",
            "images",
            "image",
            "jobs",
            "lab",
            "revision",
            "readme",
        ];

        for entry in fs::read_dir(&artifacts_dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            let name_path = PathBuf::from(&name);

            if name_str == "bin" {
                continue;
            }

            if collection_dirs.contains(&name_str.as_ref()) {
                if entry.path().is_dir() {
                    for sub in fs::read_dir(entry.path())? {
                        let sub = sub?;
                        let sub_rel = name_path.join(sub.file_name());
                        if !live_artifacts.contains(&sub_rel) {
                            log_info!("Deleting unused artifact: {:?}", sub_rel);
                            if sub.path().is_dir() {
                                if let Err(e) = fs::remove_dir_all(sub.path()) {
                                    log_warn!("Failed to delete directory {:?}: {}", sub.path(), e);
                                }
                            } else if let Err(e) = fs::remove_file(sub.path()) {
                                log_warn!("Failed to delete file {:?}: {}", sub.path(), e);
                            }
                        }
                    }
                }
            } else if !live_artifacts.contains(&name_path) {
                log_info!("Deleting unused artifact: {:?}", name);
                if entry.path().is_dir() {
                    if let Err(e) = fs::remove_dir_all(entry.path()) {
                        log_warn!("Failed to delete directory {:?}: {}", entry.path(), e);
                    }
                } else if let Err(e) = fs::remove_file(entry.path()) {
                    log_warn!("Failed to delete file {:?}: {}", entry.path(), e);
                }
            }
        }
    }

    if outputs_dir.exists() {
        for entry in fs::read_dir(&outputs_dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            if !live_jobs.contains(name_str.as_ref()) {
                log_info!("Deleting unused output: {:?}", name);
                if let Err(e) = fs::remove_dir_all(entry.path()) {
                    log_warn!("Failed to delete output {:?}: {}", entry.path(), e);
                }
            }
        }
    }

    Ok(())
}
