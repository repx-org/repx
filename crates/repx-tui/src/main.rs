mod app;
mod event;
mod model;
mod ui;
mod widgets;

use crate::app::LogPollerCommand;
use crate::{
    app::{App, SubmissionResult},
    event::handle_key_event,
};
use clap::Parser;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use repx_client::Client;
use repx_core::{
    config::{self, Resources},
    error::AppError,
    log_debug,
    model::JobId,
    theme,
};
use std::{
    fs,
    io::{self, Stdout},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use toml::Value;

#[derive(Parser)]
struct TuiArgs {
    #[arg(short, long, global = true, default_value = "./result")]
    pub lab: PathBuf,
}

fn merge_toml_values(a: &mut Value, b: &Value) {
    match (a, b) {
        (Value::Table(a), Value::Table(b)) => {
            for (k, v) in b {
                merge_toml_values(a.entry(k.clone()).or_insert(v.clone()), v);
            }
        }
        (a, b) => {
            *a = b.clone();
        }
    }
}

fn load_resources_config() -> Result<Option<Resources>, AppError> {
    let mut merged_value = Value::Table(toml::map::Map::new());

    let xdg_dirs = xdg::BaseDirectories::with_prefix("repx");
    if let Some(global_path) = xdg_dirs.find_config_file("resources.toml") {
        log_debug!("Loading global resources from: {}", global_path.display());
        let content = fs::read_to_string(global_path)?;
        let global_value: Value = toml::from_str(&content).map_err(AppError::Toml)?;
        merge_toml_values(&mut merged_value, &global_value);
    }

    let cwd_path = std::env::current_dir()?.join("resources.toml");
    if cwd_path.exists() {
        log_debug!("Loading local resources from: {}", cwd_path.display());
        let content = fs::read_to_string(cwd_path)?;
        let local_value: Value = toml::from_str(&content).map_err(AppError::Toml)?;
        merge_toml_values(&mut merged_value, &local_value);
    }
    if merged_value.as_table().is_none_or(|t| t.is_empty()) {
        Ok(None)
    } else {
        let final_resources: Resources = merged_value.try_into().map_err(AppError::Toml)?;
        Ok(Some(final_resources))
    }
}

fn main() -> Result<(), AppError> {
    repx_core::logging::set_log_level_from_env();
    if let Err(e) = repx_core::logging::init_tui_logger() {
        eprintln!("[ERROR] Failed to initialize TUI logger: {}", e);
        std::process::exit(1);
    }
    repx_core::log_info!("--- Repx TUI Started ---");

    let args = TuiArgs::parse();
    let lab_path = fs::canonicalize(&args.lab).map_err(|e| AppError::PathIo {
        path: args.lab.clone(),
        source: e,
    })?;
    let config = config::load_config()?;
    let theme = theme::load_theme(&config)?;
    let resources = load_resources_config()?;
    let client = Client::new(config.clone(), lab_path).map_err(|e| AppError::ExecutionFailed {
        message: "TUI failed to initialize client".to_string(),
        log_path: None,
        log_summary: e.to_string(),
    })?;

    let (status_tx, status_rx) = mpsc::channel();
    let status_client_clone = client.clone();
    let should_quit = Arc::new(AtomicBool::new(false));
    let should_quit_clone_for_status = should_quit.clone();

    let initial_active_target = client
        .config()
        .submission_target
        .clone()
        .unwrap_or_else(|| "local".to_string());
    let active_target = Arc::new(Mutex::new(initial_active_target.clone()));
    let active_target_clone_for_status = active_target.clone();

    thread::spawn(move || loop {
        if should_quit_clone_for_status.load(Ordering::Relaxed) {
            break;
        }

        let target_name = active_target_clone_for_status.lock().unwrap().clone();

        let statuses = status_client_clone
            .get_statuses_for_active_target(&target_name)
            .map(|job_statuses| (target_name, job_statuses));
        if status_tx.send(statuses).is_err() {
            break;
        }
        thread::sleep(Duration::from_secs(5));
    });

    let (log_cmd_tx, log_cmd_rx) = mpsc::channel::<LogPollerCommand>();
    let (log_result_tx, log_result_rx) = mpsc::channel();
    let log_client_clone = client.clone();
    let should_quit_clone_for_logs = should_quit.clone();
    let active_target_clone_for_logs = active_target.clone();

    thread::spawn(move || {
        let mut current_job_to_watch: Option<JobId> = None;
        let mut last_fetch = Instant::now();
        let polling_interval = Duration::from_secs(2);

        loop {
            if should_quit_clone_for_logs.load(Ordering::Relaxed) {
                break;
            }

            if let Ok(cmd) = log_cmd_rx.try_recv() {
                match cmd {
                    LogPollerCommand::Watch(job_id) => {
                        if current_job_to_watch.as_ref() != Some(&job_id) {
                            current_job_to_watch = Some(job_id);
                            last_fetch = Instant::now() - polling_interval;
                        }
                    }
                    LogPollerCommand::Stop => {
                        current_job_to_watch = None;
                    }
                }
            }

            if let Some(job_id) = &current_job_to_watch {
                if last_fetch.elapsed() >= polling_interval {
                    let target_name = active_target_clone_for_logs.lock().unwrap().clone();
                    let log_result =
                        log_client_clone.get_log_tail(job_id.clone(), &target_name, 50);
                    if log_result_tx.send((job_id.clone(), log_result)).is_err() {
                        break;
                    }
                    last_fetch = Instant::now();
                }
            }

            thread::sleep(Duration::from_millis(200));
        }
    });

    let (submission_tx, submission_rx) = mpsc::channel::<SubmissionResult>();

    let mut app = App::new(
        client,
        theme,
        status_rx,
        log_cmd_tx,
        log_result_rx,
        submission_tx,
        submission_rx,
        resources,
        initial_active_target,
        active_target,
    )
    .map_err(|e| AppError::ExecutionFailed {
        message: "TUI app initialization failed".to_string(),
        log_path: None,
        log_summary: e.to_string(),
    })?;

    let mut terminal = setup_terminal()?;
    run_app(&mut terminal, &mut app)?;

    should_quit.store(true, Ordering::Relaxed);

    restore_terminal(&mut terminal)?;

    Ok(())
}

fn setup_terminal() -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    Terminal::new(backend)
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> io::Result<()> {
    let mut last_tick = Instant::now();
    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        let timeout = app
            .tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if let Some(Event::Key(key)) = event::poll_event(timeout)? {
            handle_key_event(key, app);
        }

        if last_tick.elapsed() >= app.tick_rate {
            app.on_tick();
            app.check_for_updates();
            app.check_for_log_updates();
            app.check_for_submission_updates();
            last_tick = Instant::now();
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> io::Result<()> {
    repx_core::log_info!("--- Repx TUI Shutting Down ---");
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()
}
