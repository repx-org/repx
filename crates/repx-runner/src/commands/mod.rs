use repx_client::Client;
use std::path::{Path, PathBuf};

pub mod execute;
pub mod gc;
pub mod internal;
pub mod list;
pub mod log;
pub mod run;
pub mod scatter_gather;
pub mod show;
pub mod trace;

pub(crate) fn write_marker(path: &Path) -> std::io::Result<()> {
    let f = std::fs::File::create(path)?;
    f.sync_all()?;
    Ok(())
}

pub struct AppContext<'a> {
    pub lab_path: &'a PathBuf,
    pub client: &'a Client,
    pub submission_target: &'a str,
}
