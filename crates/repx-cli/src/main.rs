use clap::{Args, Parser, Subcommand};
use colored::Colorize;
use repx_runner::cli::Commands as RunnerCommands;
use std::path::PathBuf;
use std::process::Command;
use which::which;

#[derive(Parser)]
#[command(name = "repx")]
#[command(about = "Unified CLI for RepX framework")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

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
    pub scheduler: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    #[command(flatten)]
    Runner(Box<repx_runner::cli::Commands>),

    #[command(about = "Open the TUI dashboard")]
    Tui,

    #[command(about = "Visualize the experiment topology")]
    Viz(VizArgs),

    #[command(about = "Trace effective parameters for a job")]
    TraceParams(TraceParamsArgs),

    #[command(about = "Debug/Run a job locally with interactive shell")]
    DebugRun(DebugRunArgs),
}

#[derive(Args)]
struct VizArgs {
    #[arg(short, long, help = "Output file path")]
    output: Option<PathBuf>,

    #[arg(long, help = "Output format (png, pdf, svg, etc.)")]
    format: Option<String>,
}

#[derive(Args)]
struct TraceParamsArgs {
    #[arg(help = "Job ID to trace")]
    job_id: String,
}

#[derive(Args)]
struct DebugRunArgs {
    #[arg(help = "Job ID to debug")]
    job_id: String,

    #[arg(short, long, help = "Command to run (defaults to shell)")]
    command: Option<String>,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
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
        Commands::Tui => {
            let tui_args = repx_tui::TuiArgs { lab: cli.lab };
            if let Err(e) = repx_tui::run(tui_args) {
                eprintln!("{}", format!("[ERROR] {}", e).red());
                std::process::exit(1);
            }
        }
        Commands::Viz(args) => {
            let viz_args = repx_viz::VizArgs {
                lab: cli.lab,
                output: args.output,
                format: args.format,
            };
            if let Err(e) = repx_viz::run(viz_args) {
                eprintln!("{}", format!("[ERROR] {}", e).red());
                std::process::exit(1);
            }
        }
        Commands::TraceParams(args) => {
            run_python_tool("repx_py.cli.trace_params", |cmd| {
                cmd.arg(args.job_id);
                cmd.arg("--lab").arg(cli.lab);
            });
        }
        Commands::DebugRun(args) => {
            run_python_tool("repx_py.cli.debug_runner", |cmd| {
                cmd.arg(args.job_id);
                cmd.arg("--lab").arg(cli.lab);
                if let Some(c) = args.command {
                    cmd.arg("--command").arg(c);
                }
            });
        }
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
