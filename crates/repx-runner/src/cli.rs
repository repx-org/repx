use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    author,
    version,
    about = "A focused SLURM job runner for repx labs.",
    long_about = "This tool reads a repx lab definition and submits its jobs to a SLURM cluster."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    #[arg(short, long, global = true, default_value = "./result")]
    pub lab: PathBuf,

    #[arg(
        long,
        global = true,
        help = "Path to a resources.toml file for execution requirements"
    )]
    pub resources: Option<PathBuf>,

    #[arg(short, long, action = clap::ArgAction::Count, global = true, help = "Increase verbosity level (-v for debug, -vv for trace)")]
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
pub enum Commands {
    Run(RunArgs),
    Gc(GcArgs),

    #[command(hide = true)]
    InternalOrchestrate(InternalOrchestrateArgs),

    #[command(hide = true)]
    InternalExecute(InternalExecuteArgs),

    #[command(hide = true)]
    InternalScatterGather(InternalScatterGatherArgs),

    #[command(hide = true)]
    InternalGc(InternalGcArgs),

    List(ListArgs),
}

#[derive(Args)]
pub struct ListArgs {}

#[derive(Args)]
pub struct GcArgs {
    #[arg(
        long,
        help = "The target to garbage collect (must be defined in config.toml)"
    )]
    pub target: Option<String>,
}

#[derive(Args)]
pub struct InternalGcArgs {
    #[arg(long)]
    pub base_path: PathBuf,
}

#[derive(Args)]
pub struct RunArgs {
    #[arg(value_name = "RUN_OR_JOB_ID")]
    pub run_specs: Vec<String>,

    #[arg(
        short = 'j',
        long,
        help = "Set the maximum number of parallel jobs for the local scheduler."
    )]
    pub jobs: Option<usize>,
}

#[derive(Args)]
pub struct InternalOrchestrateArgs {
    #[arg(value_name = "PLAN_FILE")]
    pub plan_file: PathBuf,
}

#[derive(Args)]
pub struct InternalExecuteArgs {
    #[arg(long, help = "The ID of the job to execute.")]
    pub job_id: String,
    #[arg(long)]
    pub runtime: String,
    #[arg(long)]
    pub image_tag: Option<String>,
    #[arg(long)]
    pub base_path: PathBuf,
    #[arg(long)]
    pub node_local_path: Option<PathBuf>,
    #[arg(long)]
    pub host_tools_dir: String,
    #[arg(long)]
    pub executable_path: PathBuf,
}

#[derive(Args)]
pub struct InternalScatterGatherArgs {
    #[arg(long, help = "The ID of the composite scatter-gather job.")]
    pub job_id: String,
    #[arg(long)]
    pub runtime: String,
    #[arg(long)]
    pub image_tag: Option<String>,
    #[arg(long)]
    pub base_path: PathBuf,
    #[arg(long)]
    pub node_local_path: Option<PathBuf>,
    #[arg(long)]
    pub host_tools_dir: String,
    #[arg(long)]
    pub scheduler: String,
    #[arg(long, allow_hyphen_values = true)]
    pub worker_sbatch_opts: String,
    #[arg(long)]
    pub job_package_path: PathBuf,
    #[arg(long)]
    pub scatter_exe_path: PathBuf,
    #[arg(long)]
    pub worker_exe_path: PathBuf,
    #[arg(long)]
    pub gather_exe_path: PathBuf,
    #[arg(long)]
    pub worker_outputs_json: String,

    #[arg(long)]
    pub anchor_id: Option<u32>,

    #[arg(long, default_value = "all")]
    pub phase: String,
}
