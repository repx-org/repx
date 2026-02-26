use crate::cli::{GcArgs, GcCommand, GcPinArgs, GcUnpinArgs, InternalGcArgs};
use crate::commands::AppContext;
use crate::error::CliError;
use repx_core::{config::Config, constants::dirs, errors::DomainError, lab};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

pub fn handle_gc_dispatch(
    args: GcArgs,
    context: &AppContext,
    config: &Config,
) -> Result<(), CliError> {
    match args.command {
        None => handle_gc_collect(args.target.as_deref(), context, config),
        Some(GcCommand::List) => handle_gc_list(args.target.as_deref(), context),
        Some(GcCommand::Pin(pin_args)) => handle_gc_pin(pin_args, args.target.as_deref(), context),
        Some(GcCommand::Unpin(unpin_args)) => {
            handle_gc_unpin(unpin_args, args.target.as_deref(), context)
        }
    }
}

fn handle_gc_collect(
    target_arg: Option<&str>,
    context: &AppContext,
    _config: &Config,
) -> Result<(), CliError> {
    let target_name = target_arg.unwrap_or(context.submission_target);
    tracing::info!("Garbage collecting target '{}'...", target_name);

    let target = context
        .client
        .get_target(target_name)
        .ok_or_else(|| CliError::Domain(DomainError::TargetNotFound(target_name.to_string())))?;

    if let Err(e) = target.deploy_repx_binary() {
        tracing::warn!(
            "Failed to verify/deploy repx binary: {}. Trying to run GC anyway.",
            e
        );
    }

    match target.garbage_collect() {
        Ok(msg) => println!("{}", msg),
        Err(e) => {
            return Err(CliError::ExecutionFailed {
                message: "Failed to run GC on target".to_string(),
                log_path: None,
                log_summary: e.to_string(),
            })
        }
    }

    Ok(())
}

fn handle_gc_list(target_arg: Option<&str>, context: &AppContext) -> Result<(), CliError> {
    let target_name = target_arg.unwrap_or(context.submission_target);

    let target = context
        .client
        .get_target(target_name)
        .ok_or_else(|| CliError::Domain(DomainError::TargetNotFound(target_name.to_string())))?;

    let roots = target
        .list_gc_roots()
        .map_err(|e| CliError::ExecutionFailed {
            message: "Failed to list GC roots".to_string(),
            log_path: None,
            log_summary: e.to_string(),
        })?;

    if roots.is_empty() {
        println!("No GC roots found on target '{}'.", target_name);
        return Ok(());
    }

    println!("{:<8} {:<45} TARGET", "KIND", "NAME");
    for root in &roots {
        println!("{:<8} {:<45} {}", root.kind, root.name, root.target_path);
    }

    Ok(())
}

fn handle_gc_pin(
    args: GcPinArgs,
    target_arg: Option<&str>,
    context: &AppContext,
) -> Result<(), CliError> {
    let target_name = target_arg.unwrap_or(context.submission_target);

    let target = context
        .client
        .get_target(target_name)
        .ok_or_else(|| CliError::Domain(DomainError::TargetNotFound(target_name.to_string())))?;

    let lab_hash = match args.lab_hash {
        Some(h) => h,
        None => {
            let lab =
                lab::load_from_path(context.lab_path).map_err(|e| CliError::ExecutionFailed {
                    message: "Failed to load lab metadata. Provide a lab hash explicitly."
                        .to_string(),
                    log_path: None,
                    log_summary: e.to_string(),
                })?;
            lab.content_hash.clone()
        }
    };

    let name = args.name.unwrap_or_else(|| lab_hash.clone());

    target
        .pin_gc_root(&lab_hash, &name)
        .map_err(|e| CliError::ExecutionFailed {
            message: format!("Failed to pin GC root on target '{}'", target_name),
            log_path: None,
            log_summary: e.to_string(),
        })?;

    println!("Pinned '{}' on target '{}'.", name, target_name);
    Ok(())
}

fn handle_gc_unpin(
    args: GcUnpinArgs,
    target_arg: Option<&str>,
    context: &AppContext,
) -> Result<(), CliError> {
    let target_name = target_arg.unwrap_or(context.submission_target);

    let target = context
        .client
        .get_target(target_name)
        .ok_or_else(|| CliError::Domain(DomainError::TargetNotFound(target_name.to_string())))?;

    target
        .unpin_gc_root(&args.name)
        .map_err(|e| CliError::ExecutionFailed {
            message: format!(
                "Failed to unpin '{}' on target '{}'",
                args.name, target_name
            ),
            log_path: None,
            log_summary: e.to_string(),
        })?;

    println!("Unpinned '{}' from target '{}'.", args.name, target_name);
    Ok(())
}

pub async fn async_handle_internal_gc(args: InternalGcArgs) -> Result<(), CliError> {
    let base_path = args.base_path;
    let gcroots_dir = base_path.join(dirs::GCROOTS);
    let artifacts_dir = base_path.join(dirs::ARTIFACTS);
    let outputs_dir = base_path.join(dirs::OUTPUTS);

    if !gcroots_dir.exists() {
        tracing::info!(
            "No gcroots directory found at {}. Nothing to GC.",
            gcroots_dir.display()
        );
        return Ok(());
    }

    tracing::info!("Scanning GC roots in {}...", gcroots_dir.display());

    let mut live_artifacts = HashSet::new();
    let mut live_jobs = HashSet::new();

    let process_link = |path: PathBuf,
                        live_arts: &mut HashSet<PathBuf>,
                        live_js: &mut HashSet<String>|
     -> Result<(), CliError> {
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
                        tracing::warn!(
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

    tracing::info!(
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
            "store",
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
                            tracing::info!("Deleting unused artifact: {:?}", sub_rel);
                            if sub.path().is_dir() {
                                if let Err(e) = fs::remove_dir_all(sub.path()) {
                                    tracing::warn!(
                                        "Failed to delete directory {:?}: {}",
                                        sub.path(),
                                        e
                                    );
                                }
                            } else if let Err(e) = fs::remove_file(sub.path()) {
                                tracing::warn!("Failed to delete file {:?}: {}", sub.path(), e);
                            }
                        }
                    }
                }
            } else if !live_artifacts.contains(&name_path) {
                tracing::info!("Deleting unused artifact: {:?}", name);
                if entry.path().is_dir() {
                    if let Err(e) = fs::remove_dir_all(entry.path()) {
                        tracing::warn!("Failed to delete directory {:?}: {}", entry.path(), e);
                    }
                } else if let Err(e) = fs::remove_file(entry.path()) {
                    tracing::warn!("Failed to delete file {:?}: {}", entry.path(), e);
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
                tracing::info!("Deleting unused output: {:?}", name);
                if let Err(e) = fs::remove_dir_all(entry.path()) {
                    tracing::warn!("Failed to delete output {:?}: {}", entry.path(), e);
                }
            }
        }
    }

    Ok(())
}
