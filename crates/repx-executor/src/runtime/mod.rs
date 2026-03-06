mod bwrap;
mod container;
mod native;

pub use bwrap::BwrapRuntime;
pub use container::ContainerRuntime;
pub use native::NativeRuntime;

use crate::error::ExecutorError;
use crate::util::ImageTag;
use nix::fcntl::{Flock, FlockArg};

pub(crate) const CONTAINER_HOSTNAME: &str = "repx-container";

#[derive(Debug, Clone)]
pub enum Runtime {
    Native,
    Podman { image_tag: ImageTag },
    Docker { image_tag: ImageTag },
    Bwrap { image_tag: ImageTag },
}

impl Runtime {
    pub fn image_tag(&self) -> Option<&ImageTag> {
        match self {
            Runtime::Native => None,
            Runtime::Podman { image_tag }
            | Runtime::Docker { image_tag }
            | Runtime::Bwrap { image_tag } => Some(image_tag),
        }
    }
}

impl std::fmt::Display for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Runtime::Native => write!(f, "native"),
            Runtime::Podman { image_tag } => write!(f, "podman ({})", image_tag),
            Runtime::Docker { image_tag } => write!(f, "docker ({})", image_tag),
            Runtime::Bwrap { image_tag } => write!(f, "bwrap ({})", image_tag),
        }
    }
}

const LOCK_POLL_INTERVAL_MS: u64 = 100;
const LOCK_TIMEOUT_SECS_DEFAULT: u64 = 300;

fn lock_timeout() -> std::time::Duration {
    let secs = std::env::var("REPX_LOCK_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(LOCK_TIMEOUT_SECS_DEFAULT);
    std::time::Duration::from_secs(secs)
}

pub(crate) async fn acquire_flock(
    lock_path: &std::path::Path,
    context_name: &str,
) -> Result<Flock<std::fs::File>, ExecutorError> {
    let mut lock_file = std::fs::File::create(lock_path)?;
    let timeout = lock_timeout();
    let lock_start = std::time::Instant::now();
    loop {
        match Flock::lock(lock_file, FlockArg::LockExclusiveNonblock) {
            Ok(lock) => return Ok(lock),
            Err((f, errno))
                if errno == nix::errno::Errno::EWOULDBLOCK
                    || errno == nix::errno::Errno::EAGAIN =>
            {
                if lock_start.elapsed() > timeout {
                    return Err(ExecutorError::LockFailed(format!(
                        "Timed out waiting for {} lock after {}s (set REPX_LOCK_TIMEOUT_SECS to override)",
                        context_name,
                        timeout.as_secs()
                    )));
                }
                lock_file = f;
                tokio::time::sleep(std::time::Duration::from_millis(LOCK_POLL_INTERVAL_MS)).await;
            }
            Err((_, e)) => {
                return Err(ExecutorError::LockFailed(format!(
                    "Failed to acquire {} lock: {}",
                    context_name, e
                )))
            }
        }
    }
}
