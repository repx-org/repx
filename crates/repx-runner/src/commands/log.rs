use crate::cli::LogArgs;
use crate::commands::AppContext;
use crate::error::CliError;
use repx_client::client::LogType;
use repx_core::{lab, model::RunId, resolver};

pub fn handle_log(args: LogArgs, context: &AppContext) -> Result<(), CliError> {
    let lab = lab::load_from_path(context.lab_path)?;
    let target_input = RunId::from(args.job_id.clone());
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

        let mut last_seen_lines: Vec<String> = lines.iter().rev().take(10).cloned().collect();
        last_seen_lines.reverse();

        while running.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_secs(1));
            let new_lines = context.client.get_log_tail(
                job_id.clone(),
                context.submission_target,
                args.lines.max(200),
                log_type,
            )?;

            if new_lines.is_empty() {
                continue;
            }

            let start_idx = if last_seen_lines.is_empty() {
                0
            } else {
                let mut found_idx = None;
                for i in 0..new_lines.len() {
                    let remaining = new_lines.len() - i;
                    let check_len = last_seen_lines.len().min(remaining);
                    if check_len > 0
                        && new_lines[i..i + check_len] == last_seen_lines[..check_len]
                        && last_seen_lines.len() <= remaining
                        && new_lines[i..i + last_seen_lines.len()] == last_seen_lines[..]
                    {
                        found_idx = Some(i + last_seen_lines.len());
                        break;
                    }
                }
                found_idx.unwrap_or(0)
            };

            for line in &new_lines[start_idx..] {
                println!("{}", line);
            }

            if !new_lines.is_empty() {
                last_seen_lines = new_lines.iter().rev().take(10).cloned().collect();
                last_seen_lines.reverse();
            }
        }
    }

    Ok(())
}
