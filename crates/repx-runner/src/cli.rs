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
    #[command(about = "Submit and run jobs")]
    Run(RunArgs),
    #[command(about = "Garbage collect old runs/jobs")]
    Gc(GcArgs),

    #[command(hide = true)]
    InternalOrchestrate(InternalOrchestrateArgs),

    #[command(hide = true)]
    InternalExecute(InternalExecuteArgs),

    #[command(hide = true)]
    InternalScatterGather(InternalScatterGatherArgs),

    #[command(hide = true)]
    InternalGc(InternalGcArgs),

    #[command(about = "List runs, jobs, or dependencies")]
    List(ListArgs),

    #[command(about = "Show detailed information")]
    Show(ShowArgs),

    #[command(
        about = "Trace effective parameters for a job (includes inherited params from dependencies)"
    )]
    TraceParams(TraceParamsArgs),
}

#[derive(Args)]
pub struct ListArgs {
    #[command(subcommand)]
    pub entity: Option<ListEntity>,
}

#[derive(Args)]
pub struct ShowArgs {
    #[command(subcommand)]
    pub entity: ShowEntity,
}

#[derive(Subcommand)]
pub enum ShowEntity {
    #[command(about = "Show detailed information about a job")]
    Job(ShowJobArgs),
    #[command(about = "Show contents of a job's output file")]
    Output(ShowOutputArgs),
}

#[derive(Args)]
pub struct ShowJobArgs {
    #[arg(help = "Job ID (or prefix) to inspect")]
    pub job_id: String,
}

#[derive(Args)]
pub struct ShowOutputArgs {
    #[arg(help = "Job ID (or prefix)")]
    pub job_id: String,
    #[arg(help = "Path to the output file (relative to job's out/ directory)")]
    pub path: Option<String>,
}

#[derive(Subcommand)]
pub enum ListEntity {
    Runs {
        #[arg(required = false, value_name = "RUN_NAME")]
        name: Option<String>,
    },
    Jobs(ListJobsArgs),
    #[command(alias = "deps")]
    Dependencies {
        job_id: String,
    },
    #[command(about = "List run groups defined in the lab")]
    Groups {
        #[arg(required = false, value_name = "GROUP_NAME")]
        name: Option<String>,
    },
}

#[derive(Args)]
pub struct ListJobsArgs {
    #[arg(required = false, value_name = "RUN_NAME")]
    pub name: Option<String>,

    #[arg(
        long,
        short = 's',
        help = "Filter jobs by stage name (substring match)"
    )]
    pub stage: Option<String>,

    #[arg(long, help = "Show output directory paths for each job")]
    pub output_paths: bool,

    #[arg(
        long,
        short = 'p',
        help = "Show effective parameter value(s) for each job. Can be repeated."
    )]
    pub param: Vec<String>,

    #[arg(long, short = 'g', help = "Group jobs by stage name")]
    pub group_by_stage: bool,
}

#[derive(Args)]
pub struct GcArgs {
    #[arg(
        long,
        help = "The target to garbage collect (must be defined in config.toml)"
    )]
    pub target: Option<String>,
}

#[derive(Args)]
pub struct TraceParamsArgs {
    #[arg(help = "Job ID to trace (optional, shows all jobs if omitted)")]
    pub job_id: Option<String>,
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

    #[arg(
        long,
        help = "Continue running independent jobs even when some jobs fail. Report all failures at the end."
    )]
    pub continue_on_failure: bool,
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
    #[arg(long, default_value_t = false)]
    pub mount_host_paths: bool,
    #[arg(long)]
    pub mount_paths: Vec<String>,
    #[arg(long)]
    pub executable_path: PathBuf,
    #[arg(long, default_value_t = false, help = "Enable debug logging to stderr")]
    pub debug: bool,
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

    #[arg(long, default_value_t = false)]
    pub mount_host_paths: bool,
    #[arg(long)]
    pub mount_paths: Vec<String>,
}
