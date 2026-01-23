use repx_client::Client;
use std::path::PathBuf;

pub mod execute;
pub mod gc;
pub mod internal;
pub mod list;
pub mod run;
pub mod scatter_gather;

pub struct AppContext<'a> {
    pub lab_path: &'a PathBuf,
    pub client: &'a Client,
    pub submission_target: &'a str,
}
