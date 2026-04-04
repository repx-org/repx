use clap::{Args, CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use colored::Colorize;
use repx_core::{
    cache::{CacheStats, CacheStore, FsCache, KNOWN_CACHE_TYPES},
    model::SchedulerType,
};
use repx_runner::cli::Commands as RunnerCommands;
use std::path::{Path, PathBuf};
use std::process::Command;
use which::which;

mod init;

#[derive(Parser)]
#[command(name = "repx")]
#[command(about = "Unified CLI for RepX framework")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    #[arg(long, help = "Print help for all commands recursively")]
    help_all: bool,

    #[arg(short, long, global = true, default_value = "./result")]
    pub lab: PathBuf,

    #[arg(
        long,
        global = true,
        help = "Path to a resources.toml file for execution requirements"
    )]
    pub resources: Option<PathBuf>,

    #[arg(short, long, action = clap::ArgAction::Count, global = true, help = "Increase verbosity level")]
    pub verbose: u8,

    #[arg(
        long,
        global = true,
        help = "The target to submit the job to (must be defined in config.toml)"
    )]
    pub target: Option<String>,

    #[arg(
        long,
        global = true,
        help = "The scheduler to use: 'slurm' or 'local'. Overrides the target's configuration."
    )]
    pub scheduler: Option<SchedulerType>,
}

#[derive(Subcommand)]
enum Commands {
    #[command(flatten)]
    Runner(Box<repx_runner::cli::Commands>),

    #[command(about = "Open the TUI dashboard")]
    Tui(TuiCmdArgs),

    #[command(about = "Visualize the experiment topology")]
    Viz(VizArgs),

    #[command(about = "Debug/Run a job locally with interactive shell")]
    DebugRun(DebugRunArgs),

    #[command(about = "Initialize a new repx experiment project")]
    Init(InitArgs),

    #[command(about = "Generate shell completions")]
    Completions(CompletionsArgs),

    #[command(about = "Manage the repx cache")]
    Cache(CacheArgs),
}

#[derive(Args)]
struct VizArgs {
    #[arg(short, long, help = "Output file path")]
    output: Option<PathBuf>,

    #[arg(long, help = "Output format (png, pdf, svg, etc.)")]
    format: Option<String>,

    #[arg(
        long,
        default_value_t = true,
        help = "Draw pipeline DAG nodes and edges (default: on)"
    )]
    pipelines: bool,

    #[arg(
        long,
        default_value_t = false,
        help = "Draw run summary nodes listing their pipelines"
    )]
    runs: bool,

    #[arg(
        long,
        default_value_t = false,
        help = "Draw group cluster wrappers around run summaries (implies --runs)"
    )]
    groups: bool,

    #[arg(
        long,
        default_value_t = false,
        help = "Show varying parameter nodes on pipeline stages"
    )]
    show_params: bool,

    #[arg(
        long,
        default_value_t = false,
        help = "Hide intra-pipeline stage edges"
    )]
    no_intra_edges: bool,

    #[arg(
        long,
        default_value_t = false,
        help = "Hide inter-run dependency edges"
    )]
    no_inter_edges: bool,
}

#[derive(Args)]
struct DebugRunArgs {
    #[arg(help = "Job ID to debug")]
    job_id: String,

    #[arg(short, long, help = "Command to run (defaults to shell)")]
    command: Option<String>,
}

#[derive(Args)]
struct InitArgs {
    #[arg(default_value = ".")]
    path: PathBuf,

    #[arg(long)]
    name: Option<String>,
}

#[derive(Args)]
struct CompletionsArgs {
    #[arg(long, help = "Shell to generate completions for")]
    shell: Shell,
}

#[derive(Args)]
struct CacheArgs {
    #[command(subcommand)]
    action: CacheAction,
}

#[derive(Subcommand)]
enum CacheAction {
    #[command(about = "List all cache entries")]
    List {
        #[arg(long, help = "Filter by cache type")]
        r#type: Option<String>,
    },

    #[command(about = "Show cache statistics")]
    Stats,

    #[command(about = "Invalidate cache entries (remove metadata only)")]
    Invalidate {
        #[arg(long, help = "Invalidate all entries of this type")]
        r#type: Option<String>,

        #[arg(help = "Specific cache key to invalidate (format: type:id)")]
        key: Option<String>,

        #[arg(long, help = "Invalidate all entries")]
        all: bool,
    },

    #[command(about = "Remove cached data and metadata")]
    Clear {
        #[arg(long, help = "Remove only entries of this type")]
        r#type: Option<String>,

        #[arg(long, help = "Skip confirmation prompt")]
        yes: bool,
    },

    #[command(about = "List known cache type names")]
    Types,
}

#[derive(Args)]
struct TuiCmdArgs {
    #[arg(long)]
    screenshot: Option<PathBuf>,

    #[arg(long, default_value = "120")]
    screenshot_width: u16,

    #[arg(long, default_value = "36")]
    screenshot_height: u16,
}

fn print_help_all() {
    let cmd = Cli::command();
    print_command_help(&cmd, 0);
}

fn print_command_help(cmd: &clap::Command, depth: usize) {
    let indent = "  ".repeat(depth);
    let name = cmd.get_name();

    if cmd.is_hide_set() {
        return;
    }

    if depth == 0 {
        println!("{}", "=".repeat(60));
        println!("REPX - Complete Command Reference");
        println!("{}", "=".repeat(60));
        println!();
    } else {
        println!();
        println!("{}{}", indent, "-".repeat(50 - indent.len()));
        println!("{}Command: {}", indent, name);
        println!("{}{}", indent, "-".repeat(50 - indent.len()));
    }

    let mut help_cmd = cmd.clone();
    let help_text = help_cmd.render_help();

    for line in help_text.to_string().lines() {
        println!("{}{}", indent, line);
    }

    for subcmd in cmd.get_subcommands() {
        print_command_help(subcmd, depth + 1);
    }
}

#[allow(clippy::expect_used)]
fn main() {
    let cli = Cli::parse();

    if cli.help_all {
        print_help_all();
        return;
    }

    let command = match cli.command {
        Some(cmd) => cmd,
        None => {
            Cli::command()
                .print_help()
                .expect("printing help to stdout must succeed");
            return;
        }
    };

    repx_core::logging::set_log_level_from_env();

    if cli.verbose > 0 {
        repx_core::logging::set_log_level(repx_core::logging::LogLevel::from(cli.verbose + 1));
    }

    match command {
        Commands::Runner(cmd) => {
            let is_internal = matches!(
                cmd.as_ref(),
                RunnerCommands::InternalOrchestrate(_)
                    | RunnerCommands::InternalExecute(_)
                    | RunnerCommands::InternalScatterGather(_)
                    | RunnerCommands::InternalGc(_)
            );

            if !is_internal {
                let logging_config = repx_core::config::load_config()
                    .map(|c| c.logging)
                    .unwrap_or_default();

                if let Err(e) = repx_core::logging::init_session_logger(&logging_config) {
                    eprintln!(
                        "{}",
                        format!("[ERROR] Failed to initialize session logger: {}", e).red()
                    );
                }
            } else {
                let logging_config = repx_core::config::load_config()
                    .map(|c| c.logging)
                    .unwrap_or_default();

                if let Err(e) = repx_core::logging::init_internal_logger(&logging_config) {
                    repx_core::logging::init_stderr_logger();
                    eprintln!(
                        "{}",
                        format!(
                            "[WARN] Failed to initialize internal logger (falling back to stderr): {}",
                            e
                        )
                        .red()
                    );
                }
            }

            let runner_cli = repx_runner::cli::Cli {
                command: *cmd,
                lab: cli.lab,
                resources: cli.resources,
                verbose: cli.verbose,
                target: cli.target,
                scheduler: cli.scheduler,
            };

            if let Err(e) = repx_runner::run(runner_cli) {
                eprintln!("{}", format!("[ERROR] {}", e).red());
                std::process::exit(1);
            }
        }
        Commands::Tui(tui_cmd) => {
            let tui_args = repx_tui::TuiArgs {
                lab: cli.lab,
                screenshot: tui_cmd.screenshot,
                screenshot_width: tui_cmd.screenshot_width,
                screenshot_height: tui_cmd.screenshot_height,
            };
            if let Err(e) = repx_tui::run(tui_args) {
                eprintln!("{}", format!("[ERROR] {}", e).red());
                std::process::exit(1);
            }
        }
        Commands::Viz(args) => {
            let show_runs = args.runs || args.groups;
            let viz_args = repx_viz::VizArgs {
                lab: cli.lab,
                output: args.output,
                format: args.format,
                show_pipelines: args.pipelines,
                show_runs,
                show_groups: args.groups,
                show_params: args.show_params,
                show_intra_edges: !args.no_intra_edges,
                show_inter_edges: !args.no_inter_edges,
            };
            if let Err(e) = repx_viz::run(viz_args) {
                eprintln!("{}", format!("[ERROR] {}", e).red());
                std::process::exit(1);
            }
        }
        Commands::DebugRun(args) => {
            run_python_tool("repx_py.cli.debug_runner", |cmd| {
                cmd.arg(&args.job_id);
                cmd.arg("--lab").arg(&cli.lab);
                if let Some(c) = &args.command {
                    cmd.arg("--command").arg(c);
                }
            });
        }
        Commands::Init(args) => {
            let path = &args.path;
            let name = args.name.unwrap_or_else(|| {
                if path == Path::new(".") {
                    std::env::current_dir()
                        .ok()
                        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                        .unwrap_or_else(|| "my-experiment".to_string())
                } else {
                    path.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "my-experiment".to_string())
                }
            });
            if let Err(e) = init::handle_init(path, &name) {
                eprintln!("{}", format!("[ERROR] {}", e).red());
                std::process::exit(1);
            }
        }
        Commands::Completions(args) => {
            let mut cmd = Cli::command();
            let name = cmd.get_name().to_string();
            generate(args.shell, &mut cmd, name, &mut std::io::stdout());
        }
        Commands::Cache(args) => {
            if let Err(e) = handle_cache(args) {
                eprintln!("{}", format!("[ERROR] {}", e).red());
                std::process::exit(1);
            }
        }
    }
}

fn get_cache_root() -> Result<PathBuf, String> {
    let config =
        repx_core::config::load_config().map_err(|e| format!("Failed to load config: {}", e))?;
    let local_target = config
        .targets
        .get("local")
        .ok_or("No 'local' target configured. Cannot determine cache directory.".to_string())?;
    Ok(local_target.base_path.join("repx"))
}

fn handle_cache(args: CacheArgs) -> Result<(), String> {
    let cache_root = get_cache_root()?;
    let cache = FsCache::new(cache_root.clone());

    match args.action {
        CacheAction::List {
            r#type: type_filter,
        } => {
            let entries = cache
                .list()
                .map_err(|e| format!("Failed to list cache: {}", e))?;

            if entries.is_empty() {
                println!("No cache entries found.");
                println!("Cache root: {}", cache_root.display());
                return Ok(());
            }

            let filtered: Vec<_> = if let Some(ref filter) = type_filter {
                entries
                    .into_iter()
                    .filter(|(k, _)| k.type_name() == filter.as_str())
                    .collect()
            } else {
                entries
            };

            if filtered.is_empty() {
                if let Some(filter) = type_filter {
                    println!("No cache entries matching type '{}'.", filter);
                } else {
                    println!("No cache entries found.");
                }
                return Ok(());
            }

            println!(
                "{:<20} {:<40} {:<10} {:<12} DESCRIPTION",
                "TYPE", "KEY", "SIZE", "AGE"
            );
            println!("{}", "-".repeat(100));

            for (key, meta) in &filtered {
                let size = meta
                    .size_bytes
                    .map(format_bytes)
                    .unwrap_or_else(|| "-".to_string());

                let age = format_age(meta.created_at);
                let desc = if meta.description.len() > 30 {
                    format!("{}...", &meta.description[..27])
                } else {
                    meta.description.clone()
                };

                println!(
                    "{:<20} {:<40} {:<10} {:<12} {}",
                    key.type_name(),
                    truncate(&key.key_id(), 38),
                    size,
                    age,
                    desc
                );
            }

            println!();
            println!("Total: {} entries", filtered.len());
            println!("Cache root: {}", cache_root.display());
            Ok(())
        }

        CacheAction::Stats => {
            let entries = cache
                .list()
                .map_err(|e| format!("Failed to list cache: {}", e))?;
            let disk = cache
                .disk_usage()
                .map_err(|e| format!("Failed to calculate disk usage: {}", e))?;
            let stats = CacheStats::from_entries(&entries);

            println!("Cache Statistics");
            println!("{}", "-".repeat(40));
            println!("  Root:          {}", cache_root.display());
            println!("  Total entries: {}", stats.total_entries);
            println!("  Disk usage:    {}", format_bytes(disk));

            if let Some(oldest) = stats.oldest {
                println!("  Oldest entry:  {}", format_age(oldest));
            }
            if let Some(newest) = stats.newest {
                println!("  Newest entry:  {}", format_age(newest));
            }

            if !stats.entries_by_type.is_empty() {
                println!();
                println!("  Entries by type:");
                for (type_name, count) in &stats.entries_by_type {
                    println!("    {:<25} {}", type_name, count);
                }
            }

            Ok(())
        }

        CacheAction::Invalidate {
            r#type: type_filter,
            key,
            all,
        } => {
            if !all && type_filter.is_none() && key.is_none() {
                return Err("Specify --all, --type <name>, or a specific key.".to_string());
            }

            let entries = cache
                .list()
                .map_err(|e| format!("Failed to list cache: {}", e))?;

            let to_invalidate: Vec<_> = if all {
                entries.iter().map(|(k, _)| k).collect()
            } else if let Some(ref filter) = type_filter {
                entries
                    .iter()
                    .filter(|(k, _)| k.type_name() == filter.as_str())
                    .map(|(k, _)| k)
                    .collect()
            } else if let Some(ref key_str) = key {
                entries
                    .iter()
                    .filter(|(k, _)| k.to_string() == *key_str)
                    .map(|(k, _)| k)
                    .collect()
            } else {
                Vec::new()
            };

            if to_invalidate.is_empty() {
                println!("No matching cache entries to invalidate.");
                return Ok(());
            }

            for k in &to_invalidate {
                cache
                    .remove(k)
                    .map_err(|e| format!("Failed to remove {}: {}", k, e))?;
            }

            println!("Invalidated {} cache entries.", to_invalidate.len());
            Ok(())
        }

        CacheAction::Clear {
            r#type: type_filter,
            yes,
        } => {
            let entries = cache
                .list()
                .map_err(|e| format!("Failed to list cache: {}", e))?;

            let to_clear: Vec<_> = if let Some(ref filter) = type_filter {
                entries
                    .into_iter()
                    .filter(|(k, _)| k.type_name() == filter.as_str())
                    .collect()
            } else {
                entries
            };

            if to_clear.is_empty() {
                println!("No cache entries to clear.");
                return Ok(());
            }

            if !yes {
                println!(
                    "This will remove {} cache entries and their data.",
                    to_clear.len()
                );
                println!("Run with --yes to confirm.");
                return Ok(());
            }

            for (k, _) in &to_clear {
                if let Err(e) = cache.remove(k) {
                    eprintln!("Warning: failed to remove {}: {}", k, e);
                }
            }

            println!("Cleared {} cache entries.", to_clear.len());
            Ok(())
        }

        CacheAction::Types => {
            println!("Known cache types:");
            for type_name in KNOWN_CACHE_TYPES {
                println!("  {}", type_name);
            }
            Ok(())
        }
    }
}

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

fn format_age(timestamp: chrono::DateTime<chrono::Utc>) -> String {
    let age = chrono::Utc::now() - timestamp;
    let total_secs = age.num_seconds();
    if total_secs < 0 {
        return "future".to_string();
    }
    let days = total_secs / 86400;
    let hours = (total_secs % 86400) / 3600;
    let mins = (total_secs % 3600) / 60;
    if days > 0 {
        format!("{}d {}h", days, hours)
    } else if hours > 0 {
        format!("{}h {}m", hours, mins)
    } else {
        format!("{}m", mins)
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() > max {
        format!("{}...", &s[..max.saturating_sub(3)])
    } else {
        s.to_string()
    }
}

fn run_python_tool<F>(module: &str, setup_args: F)
where
    F: FnOnce(&mut Command),
{
    let python = if which("python3").is_ok() {
        "python3"
    } else if which("python").is_ok() {
        "python"
    } else {
        eprintln!("{}", "[ERROR] Python is not installed or not in PATH. 'repx viz/trace/debug' requires Python.".red());
        std::process::exit(1);
    };

    let mut cmd = Command::new(python);
    cmd.arg("-m").arg(module);
    setup_args(&mut cmd);

    let status = cmd.status();
    match status {
        Ok(s) => {
            if !s.success() {
                std::process::exit(s.code().unwrap_or(1));
            }
        }
        Err(e) => {
            eprintln!(
                "{}",
                format!("[ERROR] Failed to execute python tool: {}", e).red()
            );
            std::process::exit(1);
        }
    }
}
