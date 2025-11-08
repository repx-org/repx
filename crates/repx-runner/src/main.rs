use colored::Colorize;
use repx_core::{log_error, logging};
use repx_runner::run;
use std::process;

fn main() {
    logging::set_log_level_from_env();

    if let Err(e) = logging::init_cli_logger() {
        eprintln!(
            "{}",
            format!("[ERROR] Failed to initialize CLI logger: {}", e).red()
        );
        process::exit(1);
    }

    if let Err(e) = run() {
        let err_msg = format!("[ERROR] {}", e);
        log_error!("{}", err_msg);
        eprintln!("{}", err_msg.red());
        process::exit(1);
    }
}
