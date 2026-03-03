pub mod config;
pub mod constants;
pub mod engine;
pub mod errors;
pub mod fs_utils;
pub mod lab;
pub mod logging;
pub mod model;
pub mod path_safety;
pub mod resolver;
pub mod store;
pub mod theme;

use std::sync::OnceLock;

pub fn xdg_dirs() -> &'static xdg::BaseDirectories {
    static DIRS: OnceLock<xdg::BaseDirectories> = OnceLock::new();
    DIRS.get_or_init(|| xdg::BaseDirectories::with_prefix("repx"))
}
