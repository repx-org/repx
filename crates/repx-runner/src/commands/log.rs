use crate::cli::LogArgs;
use crate::commands::AppContext;
use crate::error::CliError;
use repx_client::client::LogType;
use repx_core::{lab, model::RunId, resolver};

pub fn handle_log(args: LogArgs, context: &AppContext) -> Result<(), CliError> {
    let lab = lab::load_from_path(context.lab_path)?;
    let target_input = RunId(args.job_id.clone());
    let job_id = resolver::resolve_target_job_id(&lab, &target_input)?;

    let log_type = if args.stderr {
        LogType::Stderr
    } else {
        LogType::Auto
    };

    let lines = context.client.get_log_tail(
        job_id.clone(),
        context.submission_target,
        args.lines,
        log_type,
    )?;

    for line in &lines {
        println!("{}", line);
    }

    if args.follow {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        use std::{thread, time::Duration};

        let running = Arc::new(AtomicBool::new(true));
        let r = running.clone();
        let _ = ctrlc::set_handler(move || {
            r.store(false, Ordering::SeqCst);
        });

        let mut last_line_count = lines.len();
        while running.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_secs(1));
            let new_lines = context.client.get_log_tail(
                job_id.clone(),
                context.submission_target,
                args.lines.max(200),
                log_type,
            )?;

            if new_lines.len() > last_line_count {
                for line in &new_lines[last_line_count..] {
                    println!("{}", line);
                }
                last_line_count = new_lines.len();
            }
        }
    }

    Ok(())
}
