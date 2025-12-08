use clap::Parser;
use colored::Colorize;
use repx_core::{config, log_error, logging};
use repx_runner::cli::{Cli, Commands};
use repx_runner::run;
use std::process;

fn main() {
    logging::set_log_level_from_env();

    let cli = Cli::parse();

    let is_internal = matches!(
        cli.command,
        Commands::InternalOrchestrate(_)
            | Commands::InternalExecute(_)
            | Commands::InternalScatterGather(_)
            | Commands::InternalGc(_)
    );

    if !is_internal {
        let logging_config = config::load_config().map(|c| c.logging).unwrap_or_default();

        if let Err(e) = logging::init_session_logger(&logging_config) {
            eprintln!(
                "{}",
                format!("[ERROR] Failed to initialize session logger: {}", e).red()
            );
        }
    }

    if let Err(e) = run(cli) {
        let err_msg = format!("[ERROR] {}", e);
        log_error!("{}", err_msg);
        eprintln!("{}", err_msg.red());
        process::exit(1);
    }
}
