use crate::cli::{
    GcArgs, GcCommand, GcKindFilter, GcListArgs, GcPinArgs, GcUnpinArgs, InternalGcArgs,
};
use crate::commands::AppContext;
use crate::error::CliError;
use repx_core::{config::Config, constants::dirs, errors::DomainError, lab, resolver};
use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

pub fn handle_gc_dispatch(
    args: GcArgs,
    context: &AppContext,
    config: &Config,
    verbose: repx_core::logging::Verbosity,
) -> Result<(), CliError> {
    let dry_run = args.dry_run;
    let yes = args.yes;
    let pinned_only = args.pinned_only;
    match args.command {
        None => handle_gc_collect(
            args.target.as_deref(),
            context,
            config,
            dry_run,
            yes,
            pinned_only,
            verbose,
        ),
        Some(GcCommand::List(list_args)) => {
            handle_gc_list(list_args, args.target.as_deref(), context)
        }
        Some(GcCommand::Status) => handle_gc_status(args.target.as_deref(), context),
        Some(GcCommand::Pin(pin_args)) => handle_gc_pin(pin_args, args.target.as_deref(), context),
        Some(GcCommand::Unpin(unpin_args)) => {
            handle_gc_unpin(unpin_args, args.target.as_deref(), context)
        }
    }
}

#[allow(clippy::expect_used)]
fn handle_gc_collect(
    target_arg: Option<&str>,
    context: &AppContext,
    _config: &Config,
    dry_run: bool,
    yes: bool,
    pinned_only: bool,
    verbose: repx_core::logging::Verbosity,
) -> Result<(), CliError> {
    let target_name = target_arg.unwrap_or(context.submission_target);

    let target = context
        .client
        .get_target(target_name)
        .ok_or_else(|| CliError::Domain(DomainError::TargetNotFound(target_name.to_string())))?;

    if !yes && !dry_run {
        let action = if pinned_only {
            format!(
                "This will remove all auto GC roots and garbage collect on target '{}', keeping only pinned labs.",
                target_name
            )
        } else {
            format!(
                "This will garbage collect unreferenced data on target '{}'.",
                target_name
            )
        };
        eprint!("{} Continue? [y/N] ", action);
        let mut input = String::new();
        if std::io::stdin().read_line(&mut input).is_err()
            || !input.trim().eq_ignore_ascii_case("y")
        {
            println!("Aborted.");
            return Ok(());
        }
    }

    if pinned_only {
        let removed = target
            .remove_auto_roots()
            .map_err(|e| CliError::ExecutionFailed {
                message: "Failed to remove auto GC roots".to_string(),
                log_path: None,
                log_summary: e.to_string(),
            })?;
        if removed > 0 {
            println!("Removed {} auto GC root(s).", removed);
        }
    }

    if dry_run {
        tracing::info!("[dry-run] Garbage collecting target '{}'...", target_name);
    } else {
        tracing::info!("Garbage collecting target '{}'...", target_name);
    }

    if let Err(e) = target.deploy_repx_binary() {
        tracing::warn!(
            "Failed to verify/deploy repx binary: {}. Trying to run GC anyway.",
            e
        );
    }

    match target.garbage_collect(dry_run, verbose) {
        Ok(msg) => {
            let msg = msg.trim();
            if !msg.is_empty() {
                println!("{}", msg);
            }
        }
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

fn extract_lab_hash(entry: &repx_client::targets::GcRootEntry) -> String {
    match entry.kind {
        repx_client::targets::GcRootKind::Auto => {
            if entry.name.len() > 20 && entry.name.chars().nth(19) == Some('_') {
                entry.name[20..].to_string()
            } else {
                let parts: Vec<&str> = entry.name.splitn(4, '_').collect();
                if parts.len() == 4 {
                    parts[3].to_string()
                } else {
                    "???".to_string()
                }
            }
        }
        repx_client::targets::GcRootKind::Pinned => {
            if let Some(filename) = entry
                .target_path
                .rsplit('/')
                .next()
                .or_else(|| entry.target_path.rsplit('\\').next())
            {
                if let Some(hash) = filename.strip_suffix("-lab-metadata.json") {
                    return hash.to_string();
                }
            }
            entry.name.clone()
        }
    }
}

fn handle_gc_list(
    args: GcListArgs,
    target_arg: Option<&str>,
    context: &AppContext,
) -> Result<(), CliError> {
    let target_name = target_arg.unwrap_or(context.submission_target);

    let target = context
        .client
        .get_target(target_name)
        .ok_or_else(|| CliError::Domain(DomainError::TargetNotFound(target_name.to_string())))?;

    let all_roots = target
        .list_gc_roots(args.sizes)
        .map_err(|e| CliError::ExecutionFailed {
            message: "Failed to list GC roots".to_string(),
            log_path: None,
            log_summary: e.to_string(),
        })?;

    let roots: Vec<_> = all_roots
        .into_iter()
        .filter(|root| {
            if let Some(kind_filter) = &args.kind {
                let matches = match kind_filter {
                    GcKindFilter::Auto => {
                        matches!(root.kind, repx_client::targets::GcRootKind::Auto)
                    }
                    GcKindFilter::Pinned => {
                        matches!(root.kind, repx_client::targets::GcRootKind::Pinned)
                    }
                };
                if !matches {
                    return false;
                }
            }
            if let Some(project_filter) = &args.project {
                match &root.project_id {
                    Some(pid) => {
                        if !pid.contains(project_filter.as_str()) {
                            return false;
                        }
                    }
                    None => return false,
                }
            }
            true
        })
        .collect();

    if roots.is_empty() {
        println!("No GC roots found on target '{}'.", target_name);
        return Ok(());
    }

    if args.sizes {
        println!(
            "{:<8} {:<20} {:<16} {:<10} PROJECT",
            "KIND", "NAME", "HASH", "SIZE"
        );
        let mut total_size: u64 = 0;
        for root in &roots {
            let hash = extract_lab_hash(root);
            let hash_display = truncate_hash(&hash, 14);
            let size_str = root
                .size_bytes
                .map(|s| {
                    total_size += s;
                    format_bytes(s)
                })
                .unwrap_or_else(|| "-".to_string());
            let project = root
                .project_id
                .as_deref()
                .map(|p| truncate_str(p, 16))
                .unwrap_or_else(|| "-".to_string());
            let name_display = truncate_str(&root.name, 18);
            println!(
                "{:<8} {:<20} {:<16} {:<10} {}",
                root.kind, name_display, hash_display, size_str, project
            );
        }
        println!();
        println!(
            "{} root(s), total: {}",
            roots.len(),
            format_bytes(total_size)
        );
    } else {
        println!("{:<8} {:<20} {:<16} PROJECT", "KIND", "NAME", "HASH");
        for root in &roots {
            let hash = extract_lab_hash(root);
            let hash_display = truncate_hash(&hash, 14);
            let project = root
                .project_id
                .as_deref()
                .map(|p| truncate_str(p, 16))
                .unwrap_or_else(|| "-".to_string());
            let name_display = truncate_str(&root.name, 18);
            println!(
                "{:<8} {:<20} {:<16} {}",
                root.kind, name_display, hash_display, project
            );
        }
        println!();
        println!("{} root(s). Use --sizes to see disk usage.", roots.len());
    }

    Ok(())
}

fn handle_gc_status(target_arg: Option<&str>, context: &AppContext) -> Result<(), CliError> {
    let target_name = target_arg.unwrap_or(context.submission_target);

    let target = context
        .client
        .get_target(target_name)
        .ok_or_else(|| CliError::Domain(DomainError::TargetNotFound(target_name.to_string())))?;

    let current_lab = lab::load(context.source).map_err(|e| CliError::ExecutionFailed {
        message: "Failed to load lab metadata".to_string(),
        log_path: None,
        log_summary: e.to_string(),
    })?;
    let lab_hash = &current_lab.content_hash;
    let hash_short = truncate_hash(lab_hash, 12);

    let roots = target
        .list_gc_roots(false)
        .map_err(|e| CliError::ExecutionFailed {
            message: "Failed to list GC roots".to_string(),
            log_path: None,
            log_summary: e.to_string(),
        })?;

    let references_lab = |r: &repx_client::targets::GcRootEntry| -> bool {
        r.name.contains(lab_hash.as_str()) || r.target_path.contains(lab_hash.as_str())
    };

    let pinned_matches: Vec<_> = roots
        .iter()
        .filter(|r| matches!(r.kind, repx_client::targets::GcRootKind::Pinned) && references_lab(r))
        .collect();

    let auto_matches: Vec<_> = roots
        .iter()
        .filter(|r| matches!(r.kind, repx_client::targets::GcRootKind::Auto) && references_lab(r))
        .collect();

    if !pinned_matches.is_empty() {
        let names: Vec<_> = pinned_matches.iter().map(|r| r.name.as_str()).collect();
        println!(
            "Lab {} is pinned as '{}' on target '{}'.",
            hash_short,
            names.join("', '"),
            target_name
        );
    } else {
        println!(
            "Lab {} is not pinned on target '{}'.",
            hash_short, target_name
        );
    }

    if !auto_matches.is_empty() {
        println!(
            "Lab {} has {} auto root(s) on target '{}'.",
            hash_short,
            auto_matches.len(),
            target_name
        );
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
            let lab = lab::load(context.source).map_err(|e| CliError::ExecutionFailed {
                message: "Failed to load lab metadata. Provide a lab hash explicitly.".to_string(),
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

    let roots = target
        .list_gc_roots(false)
        .map_err(|e| CliError::ExecutionFailed {
            message: "Failed to list GC roots".to_string(),
            log_path: None,
            log_summary: e.to_string(),
        })?;

    let pinned_names: Vec<&str> = roots
        .iter()
        .filter(|r| matches!(r.kind, repx_client::targets::GcRootKind::Pinned))
        .map(|r| r.name.as_str())
        .collect();

    let resolved_name =
        resolver::resolve_name_by_prefix(pinned_names, &args.name).map_err(CliError::Domain)?;

    target
        .unpin_gc_root(resolved_name)
        .map_err(|e| CliError::ExecutionFailed {
            message: format!(
                "Failed to unpin '{}' on target '{}'",
                resolved_name, target_name
            ),
            log_path: None,
            log_summary: e.to_string(),
        })?;

    println!(
        "Unpinned '{}' from target '{}'.",
        resolved_name, target_name
    );
    Ok(())
}

pub async fn async_handle_internal_gc(args: InternalGcArgs) -> Result<(), CliError> {
    let base_path = args.base_path;
    let dry_run = args.dry_run;
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

    #[allow(clippy::expect_used)]
    let process_link = |path: PathBuf,
                        live_arts: &mut HashSet<PathBuf>,
                        live_js: &mut HashSet<String>|
     -> Result<(), CliError> {
        if let Ok(target) = fs::read_link(&path) {
            let abs_target = if target.is_absolute() {
                target
            } else {
                path.parent()
                    .expect("symlink path must have a parent directory")
                    .join(target)
            };

            if let Ok(canonical) = fs::canonicalize(&abs_target) {
                if canonical.starts_with(&artifacts_dir) {
                    if let Ok(rel) = canonical.strip_prefix(&artifacts_dir) {
                        live_arts.insert(rel.to_path_buf());
                    }
                    let lab_root = canonical.clone();

                    if let Ok(lab) = lab::load_from_path_unchecked(&lab_root) {
                        for job_id in lab.jobs.keys() {
                            live_js.insert(job_id.as_str().to_owned());
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

    let mut deleted_artifacts: u64 = 0;
    let mut deleted_outputs: u64 = 0;
    let mut freed_bytes: u64 = 0;

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

        let top_entries: Vec<_> = fs::read_dir(&artifacts_dir)?
            .filter_map(|e| e.ok())
            .collect();

        let mut dead_top: Vec<&fs::DirEntry> = Vec::new();

        for entry in &top_entries {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            let name_path = PathBuf::from(&name);

            if name_str == "bin" {
                continue;
            }

            if collection_dirs.contains(&name_str.as_ref()) {
                if entry.path().is_dir() {
                    let subs: Vec<_> = fs::read_dir(entry.path())?.filter_map(|e| e.ok()).collect();
                    let dead_subs: Vec<_> = subs
                        .iter()
                        .filter(|sub| {
                            let sub_rel = name_path.join(sub.file_name());
                            !live_artifacts.contains(&sub_rel)
                        })
                        .collect();
                    if !dead_subs.is_empty() {
                        with_writable_dir(&entry.path(), || {
                            for sub in &dead_subs {
                                let sub_rel = name_path.join(sub.file_name());
                                let size = path_size(&sub.path());
                                if dry_run {
                                    tracing::info!(
                                        "[dry-run] Would delete artifact: {:?} ({})",
                                        sub_rel,
                                        format_bytes(size)
                                    );
                                    deleted_artifacts += 1;
                                    freed_bytes += size;
                                } else {
                                    tracing::info!("Deleting unused artifact: {:?}", sub_rel);
                                    if force_remove_no_parent(&sub.path()) {
                                        deleted_artifacts += 1;
                                        freed_bytes += size;
                                    }
                                }
                            }
                        });
                    }
                }
            } else if !live_artifacts.contains(&name_path) {
                dead_top.push(entry);
            }
        }

        if !dead_top.is_empty() {
            with_writable_dir(&artifacts_dir, || {
                for entry in &dead_top {
                    let name = entry.file_name();
                    let size = path_size(&entry.path());
                    if dry_run {
                        tracing::info!(
                            "[dry-run] Would delete artifact: {:?} ({})",
                            name,
                            format_bytes(size)
                        );
                        deleted_artifacts += 1;
                        freed_bytes += size;
                    } else {
                        tracing::info!("Deleting unused artifact: {:?}", name);
                        if force_remove_no_parent(&entry.path()) {
                            deleted_artifacts += 1;
                            freed_bytes += size;
                        }
                    }
                }
            });
        }
    }

    if outputs_dir.exists() {
        let output_entries: Vec<_> = fs::read_dir(&outputs_dir)?.filter_map(|e| e.ok()).collect();
        let dead_outputs: Vec<_> = output_entries
            .iter()
            .filter(|entry| {
                let name_str = entry.file_name();
                !live_jobs.contains(name_str.to_string_lossy().as_ref())
            })
            .collect();
        if !dead_outputs.is_empty() {
            with_writable_dir(&outputs_dir, || {
                for entry in &dead_outputs {
                    let name = entry.file_name();
                    let size = path_size(&entry.path());
                    if dry_run {
                        tracing::info!(
                            "[dry-run] Would delete output: {:?} ({})",
                            name,
                            format_bytes(size)
                        );
                        deleted_outputs += 1;
                        freed_bytes += size;
                    } else {
                        tracing::info!("Deleting unused output: {:?}", name);
                        if force_remove_no_parent(&entry.path()) {
                            deleted_outputs += 1;
                            freed_bytes += size;
                        }
                    }
                }
            });
        }
    }

    if dry_run {
        if deleted_artifacts == 0 && deleted_outputs == 0 {
            println!("Nothing to collect.");
        } else {
            println!(
                "Would delete {} artifact(s) and {} job output(s). Would free {}.",
                deleted_artifacts,
                deleted_outputs,
                format_bytes(freed_bytes)
            );
        }
    } else if deleted_artifacts == 0 && deleted_outputs == 0 {
        println!("Nothing to collect.");
    } else {
        println!(
            "Deleted {} artifact(s) and {} job output(s). Freed {}.",
            deleted_artifacts,
            deleted_outputs,
            format_bytes(freed_bytes)
        );
    }

    Ok(())
}

fn force_remove_no_parent(path: &std::path::Path) -> bool {
    let result = if path.is_dir() {
        for entry in walkdir::WalkDir::new(path)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_dir() {
                let _ = fs::set_permissions(entry.path(), PermissionsExt::from_mode(0o755));
            }
        }
        fs::remove_dir_all(path)
    } else {
        let _ = fs::set_permissions(path, PermissionsExt::from_mode(0o644));
        fs::remove_file(path)
    };

    match result {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!("Failed to delete {:?}: {}", path, e);
            false
        }
    }
}

fn with_writable_dir<F>(dir: &std::path::Path, f: F)
where
    F: FnOnce(),
{
    let saved = fs::metadata(dir).ok().map(|m| m.permissions());
    let _ = fs::set_permissions(dir, PermissionsExt::from_mode(0o755));
    f();
    if let Some(perms) = saved {
        let _ = fs::set_permissions(dir, perms);
    }
}

fn path_size(path: &std::path::Path) -> u64 {
    if !path.exists() {
        return 0;
    }
    if path.is_file() || path.is_symlink() {
        return fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    }
    walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.metadata().map(|m| m.len()).unwrap_or(0))
        .sum()
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

fn truncate_hash(hash: &str, max_len: usize) -> String {
    if hash.len() <= max_len {
        hash.to_string()
    } else {
        format!("{}..", &hash[..max_len - 2])
    }
}

fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}..", &s[..max_len - 2])
    }
}
