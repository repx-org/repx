mod bwrap;
mod container;
mod native;

pub use bwrap::BwrapRuntime;
pub use container::ContainerRuntime;
pub use native::NativeRuntime;

use crate::error::ExecutorError;
use nix::fcntl::{Flock, FlockArg};

#[derive(Debug, Clone)]
pub enum Runtime {
    Native,
    Podman { image_tag: String },
    Docker { image_tag: String },
    Bwrap { image_tag: String },
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
                    return Err(ExecutorError::Io(std::io::Error::other(format!(
                        "Timed out waiting for {} lock after {}s (set REPX_LOCK_TIMEOUT_SECS to override)",
                        context_name,
                        timeout.as_secs()
                    ))));
                }
                lock_file = f;
                tokio::time::sleep(std::time::Duration::from_millis(LOCK_POLL_INTERVAL_MS)).await;
            }
            Err((_, e)) => {
                return Err(ExecutorError::Io(std::io::Error::other(format!(
                    "Failed to acquire {} lock: {}",
                    context_name, e
                ))))
            }
        }
    }
}
