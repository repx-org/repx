use crate::context::RuntimeContext;
use crate::error::{ExecutorError, Result};
use crate::ExecutionRequest;
use nix::fcntl::{Flock, FlockArg};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command as TokioCommand;

const EXCLUDED_ROOTFS_DIRS: &[&str] = &["dev", "proc", "tmp"];

#[derive(Debug, Serialize, Deserialize)]
struct OverlayCapabilityCache {
    tmp_overlay_supported: bool,
    checked_at: String,
}

pub struct BwrapRuntime;

const EXCLUDED_HOST_DIRS: &[&str] = &["dev", "proc", "sys", "nix"];
const WRITABLE_HOST_DIRS: &[&str] = &["home", "tmp", "var", "opt", "srv", "mnt", "media", "run"];

impl BwrapRuntime {
    pub async fn ensure_rootfs_extracted(
        ctx: &RuntimeContext<'_>,
        image_tag: &str,
    ) -> Result<PathBuf> {
        let image_hash = image_tag.split(':').next_back().unwrap_or(image_tag);

        let images_cache_dir = ctx.get_images_cache_dir();
        let image_dir = images_cache_dir.join(image_hash);
        let extract_dir = image_dir.join("rootfs");
        let success_marker = image_dir.join("SUCCESS");

        let temp_path = ctx.get_temp_path();
        let lock_path = temp_path.join(format!("repx-extract-{}.lock", image_hash));

        tokio::fs::create_dir_all(&images_cache_dir).await?;

        if success_marker.exists() && extract_dir.exists() {
            return Ok(extract_dir);
        }

        let mut lock_file = std::fs::File::create(&lock_path)?;
        let _lock = loop {
            match Flock::lock(lock_file, FlockArg::LockExclusiveNonblock) {
                Ok(lock) => break lock,
                Err((f, errno))
                    if errno == nix::errno::Errno::EWOULDBLOCK
                        || errno == nix::errno::Errno::EAGAIN =>
                {
                    lock_file = f;
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
                Err((_, e)) => {
                    return Err(ExecutorError::Io(std::io::Error::other(format!(
                        "Failed to acquire extraction lock: {}",
                        e
                    ))))
                }
            }
        };

        if success_marker.exists() && extract_dir.exists() {
            return Ok(extract_dir);
        }

        tracing::info!(
            "Extracting rootfs for image '{}' to {:?}",
            image_tag,
            extract_dir
        );

        let image_path = ctx.find_image_file(image_tag).ok_or_else(|| {
            ExecutorError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!(
                    "Image file for tag '{}' not found in artifacts/images or artifacts/image",
                    image_tag
                ),
            ))
        })?;

        if !image_path.is_dir() {
            return Err(ExecutorError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "Image artifact at {:?} must be a directory (exploded OCI layout), but it is a file.",
                    image_path
                ),
            )));
        }

        let manifest_path = image_path.join("manifest.json");
        if !manifest_path.exists() {
            return Err(ExecutorError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!(
                    "Could not find 'manifest.json' inside the image directory {:?}.",
                    image_path
                ),
            )));
        }

        #[derive(Deserialize)]
        struct ManifestEntry {
            #[serde(rename = "Layers")]
            layers: Vec<String>,
        }

        let manifest_content = tokio::fs::read_to_string(&manifest_path).await?;
        let manifest: Vec<ManifestEntry> =
            serde_json::from_str(&manifest_content).map_err(|e| {
                ExecutorError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
            })?;

        if manifest.is_empty() {
            return Err(ExecutorError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "manifest.json is empty or invalid array",
            )));
        }

        let layers = &manifest[0].layers;

        if extract_dir.exists() {
            tokio::fs::remove_dir_all(&extract_dir).await?;
        }
        tokio::fs::create_dir_all(&extract_dir).await?;

        let tar_path = ctx.resolve_tool("tar")?;

        for layer in layers {
            let layer_path = image_path.join(layer);
            if !layer_path.exists() {
                tracing::debug!(
                    "Layer {} listed in manifest but not found at {:?}, skipping.",
                    layer,
                    layer_path
                );
                continue;
            }

            tracing::debug!("Extracting layer: {:?}", layer_path);

            let mut cmd_layer = TokioCommand::new(&tar_path);
            cmd_layer
                .arg("-xf")
                .arg(&layer_path)
                .arg("-C")
                .arg(&extract_dir)
                .arg("--no-same-owner")
                .arg("--no-same-permissions")
                .arg("--mode=0755")
                .arg("--delay-directory-restore")
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());

            ctx.restrict_command_environment(&mut cmd_layer, &[]);

            let output = cmd_layer.output().await?;
            if !output.status.success() {
                let _ = tokio::fs::remove_dir_all(&extract_dir).await;
                return Err(ExecutorError::Io(std::io::Error::other(format!(
                    "Failed to extract layer '{}'. Stderr: {}",
                    layer,
                    String::from_utf8_lossy(&output.stderr)
                ))));
            }
        }

        for dir in &["dev", "proc", "tmp"] {
            let p = extract_dir.join(dir);
            if !p.exists() {
                tokio::fs::create_dir(&p).await?;
            }
        }

        std::fs::File::create(&success_marker).map_err(ExecutorError::Io)?;

        let _ = tokio::fs::remove_file(&lock_path).await;

        tracing::info!("Successfully extracted rootfs for '{}'", image_tag);
        Ok(extract_dir)
    }

    pub async fn prepare_symlink_union(
        request: &ExecutionRequest,
        host_store: &Path,
        image_store: &Path,
        host_mount_point: &Path,
        image_mount_point: &Path,
    ) -> Result<PathBuf> {
        let union_dir = request.repx_out_dir.join("nix_union_store");
        if union_dir.exists() {
            tokio::fs::remove_dir_all(&union_dir).await?;
        }
        tokio::fs::create_dir_all(&union_dir).await?;

        let mut host_entries = tokio::fs::read_dir(host_store).await?;
        while let Some(entry) = host_entries.next_entry().await? {
            let file_name = entry.file_name();
            let target = host_mount_point.join(&file_name);
            let link_path = union_dir.join(&file_name);
            tokio::fs::symlink(target, link_path).await?;
        }

        let mut image_entries = tokio::fs::read_dir(image_store).await?;
        while let Some(entry) = image_entries.next_entry().await? {
            let file_name = entry.file_name();
            let target = image_mount_point.join(&file_name);
            let link_path = union_dir.join(&file_name);

            if link_path.exists() || tokio::fs::symlink_metadata(&link_path).await.is_ok() {
                tokio::fs::remove_file(&link_path).await?;
            }
            tokio::fs::symlink(target, link_path).await?;
        }

        Ok(union_dir)
    }

    pub async fn check_overlay_support(ctx: &RuntimeContext<'_>, _lower_dir: &Path) -> bool {
        let cache_dir = ctx.get_capabilities_cache_dir();
        let cache_file = cache_dir.join("overlay_support.json");

        if let Ok(content) = tokio::fs::read_to_string(&cache_file).await {
            if let Ok(cached) = serde_json::from_str::<OverlayCapabilityCache>(&content) {
                return cached.tmp_overlay_supported;
            }
        }

        Self::run_overlay_check(ctx).await
    }

    async fn run_overlay_check(ctx: &RuntimeContext<'_>) -> bool {
        let bwrap_path = match ctx.get_host_tool_path("bwrap") {
            Ok(p) => p,
            Err(_) => return false,
        };

        let temp_base = ctx.get_temp_path();
        let temp_dir = match tempfile::Builder::new()
            .prefix(".repx-overlay-check-")
            .tempdir_in(&temp_base)
        {
            Ok(t) => t,
            Err(e) => {
                tracing::debug!("Failed to create temp dir for overlay check: {}", e);
                return false;
            }
        };

        let start_path = temp_dir.path();
        let upper = start_path.join("upper");
        let work = start_path.join("work");
        let merged = start_path.join("merged");
        let lower = start_path.join("lower");

        if tokio::fs::create_dir(&upper).await.is_err()
            || tokio::fs::create_dir(&work).await.is_err()
            || tokio::fs::create_dir(&merged).await.is_err()
            || tokio::fs::create_dir(&lower).await.is_err()
        {
            return false;
        }

        let mut cmd = TokioCommand::new(&bwrap_path);

        cmd.arg("--unshare-user")
            .arg("--dev-bind")
            .arg("/")
            .arg("/")
            .arg("--overlay-src")
            .arg(&lower)
            .arg("--overlay")
            .arg(&upper)
            .arg(&work)
            .arg(&merged)
            .arg("true");

        ctx.restrict_command_environment(&mut cmd, &[]);
        cmd.stdout(Stdio::null()).stderr(Stdio::null());

        match cmd.status().await {
            Ok(status) => status.success(),
            Err(_) => false,
        }
    }

    pub async fn check_tmp_overlay_support(ctx: &RuntimeContext<'_>, rootfs_path: &Path) -> bool {
        let cache_dir = ctx.get_capabilities_cache_dir();
        let cache_file = cache_dir.join("overlay_support.json");

        if let Ok(content) = tokio::fs::read_to_string(&cache_file).await {
            if let Ok(cached) = serde_json::from_str::<OverlayCapabilityCache>(&content) {
                tracing::debug!(
                    "Using cached overlay support result: supported={}",
                    cached.tmp_overlay_supported
                );
                return cached.tmp_overlay_supported;
            }
        }

        let supported = Self::run_tmp_overlay_check(ctx, rootfs_path).await;

        if let Err(e) = tokio::fs::create_dir_all(&cache_dir).await {
            tracing::debug!("Failed to create capabilities cache dir: {}", e);
        } else {
            let cache_entry = OverlayCapabilityCache {
                tmp_overlay_supported: supported,
                checked_at: chrono::Utc::now().to_rfc3339(),
            };
            if let Ok(json) = serde_json::to_string_pretty(&cache_entry) {
                if let Err(e) = tokio::fs::write(&cache_file, json).await {
                    tracing::debug!("Failed to write overlay capability cache: {}", e);
                }
            }
        }

        supported
    }

    async fn run_tmp_overlay_check(ctx: &RuntimeContext<'_>, rootfs_path: &Path) -> bool {
        let bwrap_path = match ctx.get_host_tool_path("bwrap") {
            Ok(p) => p,
            Err(_) => return false,
        };

        let temp_base = ctx.get_temp_path();
        let temp_dir = match tempfile::Builder::new()
            .prefix(".repx-tmp-overlay-check-")
            .tempdir_in(&temp_base)
        {
            Ok(t) => t,
            Err(e) => {
                tracing::debug!("Failed to create temp dir for tmp-overlay check: {}", e);
                return false;
            }
        };

        let test_lower = if rootfs_path.join("bin").exists() {
            rootfs_path.join("bin")
        } else if rootfs_path.join("etc").exists() {
            rootfs_path.join("etc")
        } else {
            rootfs_path.to_path_buf()
        };

        let test_mount_point = temp_dir.path().join("test");
        if tokio::fs::create_dir(&test_mount_point).await.is_err() {
            return false;
        }

        let mut cmd = TokioCommand::new(&bwrap_path);

        cmd.arg("--unshare-user")
            .arg("--dev-bind")
            .arg("/")
            .arg("/")
            .arg("--overlay-src")
            .arg(&test_lower)
            .arg("--tmp-overlay")
            .arg(&test_mount_point)
            .arg("true");

        ctx.restrict_command_environment(&mut cmd, &[]);
        cmd.stdout(Stdio::null()).stderr(Stdio::null());

        match cmd.status().await {
            Ok(status) => {
                let supported = status.success();
                if !supported {
                    tracing::debug!(
                        "tmp-overlay check failed for {:?} - kernel may not support userxattr overlay",
                        test_lower
                    );
                }
                supported
            }
            Err(e) => {
                tracing::debug!("tmp-overlay check command failed: {}", e);
                false
            }
        }
    }

    pub async fn build_command(
        ctx: &RuntimeContext<'_>,
        rootfs_path: &Path,
        script_path: &Path,
        args: &[String],
    ) -> Result<TokioCommand> {
        let bwrap_path = ctx.get_host_tool_path("bwrap")?;
        let mut cmd = TokioCommand::new(bwrap_path);
        let request = ctx.request;

        if request.mount_host_paths {
            Self::configure_host_path_mounts(&mut cmd, ctx, rootfs_path).await?;

            cmd.arg("--unshare-user")
                .arg("--unshare-pid")
                .arg("--unshare-ipc")
                .arg("--unshare-uts")
                .arg("--dev-bind")
                .arg("/dev")
                .arg("/dev")
                .arg("--proc")
                .arg("/proc");
        } else {
            cmd.arg("--unshare-all")
                .arg("--hostname")
                .arg("repx-container");

            let overlay_supported = Self::check_tmp_overlay_support(ctx, rootfs_path).await;

            if overlay_supported {
                cmd.arg("--overlay-src")
                    .arg(rootfs_path)
                    .arg("--tmp-overlay")
                    .arg("/");
            } else {
                tracing::info!(
                    "Overlay filesystem not supported on target (kernel may lack userxattr support). \
                     Using read-only bind mounts for rootfs."
                );

                Self::configure_readonly_rootfs_mounts(&mut cmd, rootfs_path).await?;
            }

            cmd.arg("--dev")
                .arg("/dev")
                .arg("--proc")
                .arg("/proc")
                .arg("--tmpfs")
                .arg("/tmp");

            cmd.arg("--dir")
                .arg(&request.base_path)
                .arg("--ro-bind")
                .arg(&request.base_path)
                .arg(&request.base_path);

            cmd.arg("--dir")
                .arg(&request.user_out_dir)
                .arg("--bind")
                .arg(&request.user_out_dir)
                .arg(&request.user_out_dir);

            cmd.arg("--dir")
                .arg(&request.repx_out_dir)
                .arg("--bind")
                .arg(&request.repx_out_dir)
                .arg(&request.repx_out_dir);

            let canonical_job_path = request
                .job_package_path
                .canonicalize()
                .unwrap_or_else(|_| request.job_package_path.clone());

            cmd.arg("--dir")
                .arg(&request.job_package_path)
                .arg("--ro-bind")
                .arg(&canonical_job_path)
                .arg(&request.job_package_path);
        }

        cmd.arg("--clearenv");

        let mut inner_path =
            String::from("/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin");
        if let Some(host_tools) = &request.host_tools_bin_dir {
            inner_path = format!("{}:{}", host_tools.display(), inner_path);
        }

        if request.mount_host_paths {
            let host_path = std::env::var("PATH").unwrap_or_default();
            if !host_path.is_empty() {
                inner_path = format!("{}:{}", inner_path, host_path);
            }
            cmd.arg("--setenv")
                .arg("HOME")
                .arg(std::env::var("HOME").unwrap_or_else(|_| "/".into()));
        } else {
            cmd.arg("--setenv").arg("HOME").arg("/");

            if !request.mount_paths.is_empty() {
                tracing::info!(
                    "[IMPURE] Specific host paths mounted: {:?}",
                    request.mount_paths
                );
                for path in &request.mount_paths {
                    cmd.arg("--bind").arg(path).arg(path);
                }
            }
        }

        cmd.arg("--setenv").arg("PATH").arg(inner_path);
        cmd.arg("--setenv").arg("TERM").arg("xterm");

        cmd.arg("--chdir").arg(&request.user_out_dir);
        cmd.arg("--");
        cmd.arg(script_path);
        cmd.args(args);

        ctx.restrict_command_environment(&mut cmd, &[]);

        tracing::info!(
            job_id = %request.job_id,
            mount_host_paths = request.mount_host_paths,
            job_package_path = %request.job_package_path.display(),
            script_path = %script_path.display(),
            rootfs_path = %rootfs_path.display(),
            "Building bwrap command"
        );
        tracing::debug!(command = ?cmd.as_std(), "Full bwrap command");

        Ok(cmd)
    }

    async fn configure_host_path_mounts(
        cmd: &mut TokioCommand,
        ctx: &RuntimeContext<'_>,
        rootfs_path: &Path,
    ) -> Result<()> {
        let request = ctx.request;
        let root = Path::new("/");
        let exclude_dirs = EXCLUDED_HOST_DIRS;
        let writable_dirs = WRITABLE_HOST_DIRS;

        if let Ok(entries) = std::fs::read_dir(root) {
            for entry in entries.flatten() {
                if let Ok(file_type) = entry.file_type() {
                    if file_type.is_dir() {
                        let dir_name = entry.file_name();
                        let dir_name_str = dir_name.to_string_lossy();

                        if exclude_dirs.contains(&dir_name_str.as_ref()) {
                            continue;
                        }

                        let dir_path = entry.path();
                        if writable_dirs.contains(&dir_name_str.as_ref()) {
                            cmd.arg("--bind").arg(&dir_path).arg(&dir_path);
                        } else {
                            cmd.arg("--ro-bind").arg(&dir_path).arg(&dir_path);
                        }
                    }
                }
            }
        }

        if Path::new("/nix/store").exists() {
            let image_store = rootfs_path.join("nix/store");
            let mut has_image_store_entries = false;

            if image_store.exists() {
                let mut entries = tokio::fs::read_dir(&image_store).await?;
                if entries.next_entry().await?.is_some() {
                    has_image_store_entries = true;
                }
            }

            let can_overlay = if has_image_store_entries {
                Self::check_overlay_support(ctx, Path::new("/nix/store")).await
            } else {
                false
            };

            if has_image_store_entries && can_overlay {
                let overlay_upper = request.repx_out_dir.join("nix_overlay_upper");
                let overlay_work = request.repx_out_dir.join("nix_overlay_work");

                tokio::fs::create_dir_all(&overlay_upper).await?;
                tokio::fs::create_dir_all(&overlay_work).await?;

                cmd.arg("--overlay-src")
                    .arg("/nix/store")
                    .arg("--overlay")
                    .arg(&overlay_upper)
                    .arg(&overlay_work)
                    .arg("/nix/store");

                let mut entries = tokio::fs::read_dir(&image_store).await?;
                while let Some(entry) = entries.next_entry().await? {
                    let file_name = entry.file_name();
                    let image_path = entry.path();
                    let target_path = PathBuf::from("/nix/store").join(file_name);
                    cmd.arg("--ro-bind").arg(image_path).arg(target_path);
                }
            } else if has_image_store_entries {
                tracing::info!(
                    "[WARN] Overlayfs not supported. Falling back to Symlink Union strategy."
                );

                let host_mount_point = request.repx_out_dir.join("store_host");
                let image_mount_point = request.repx_out_dir.join("store_image");

                tokio::fs::create_dir_all(&host_mount_point).await?;
                tokio::fs::create_dir_all(&image_mount_point).await?;

                let union_dir = Self::prepare_symlink_union(
                    request,
                    Path::new("/nix/store"),
                    &image_store,
                    &host_mount_point,
                    &image_mount_point,
                )
                .await?;

                cmd.arg("--ro-bind")
                    .arg("/nix/store")
                    .arg(&host_mount_point);

                cmd.arg("--ro-bind")
                    .arg(&image_store)
                    .arg(&image_mount_point);

                cmd.arg("--ro-bind").arg(&union_dir).arg("/nix/store");
            } else {
                cmd.arg("--ro-bind").arg("/nix/store").arg("/nix/store");
            }
        } else {
            let image_nix = rootfs_path.join("nix");
            if image_nix.exists() {
                cmd.arg("--dir").arg("/nix");
                cmd.arg("--bind").arg(image_nix).arg("/nix");
            }
        }

        Ok(())
    }

    async fn configure_readonly_rootfs_mounts(
        cmd: &mut TokioCommand,
        rootfs_path: &Path,
    ) -> Result<()> {
        let entries = match std::fs::read_dir(rootfs_path) {
            Ok(e) => e,
            Err(e) => {
                return Err(ExecutorError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("Failed to read rootfs directory {:?}: {}", rootfs_path, e),
                )));
            }
        };

        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let file_name_str = file_name.to_string_lossy();

            if EXCLUDED_ROOTFS_DIRS.contains(&file_name_str.as_ref()) {
                continue;
            }

            let source_path = entry.path();
            let target_path = format!("/{}", file_name_str);

            cmd.arg("--ro-bind").arg(&source_path).arg(&target_path);
        }

        Ok(())
    }
}
