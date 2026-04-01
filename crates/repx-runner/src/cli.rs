use clap::{Args, Parser, Subcommand, ValueEnum};
use repx_core::model::{ExecutionType, SchedulerType};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum StatusFilter {
    Succeeded,
    Failed,
    Pending,
    Running,
    Queued,
    Blocked,
}

impl StatusFilter {
    pub fn as_str(&self) -> &'static str {
        match self {
            StatusFilter::Succeeded => "succeeded",
            StatusFilter::Failed => "failed",
            StatusFilter::Pending => "pending",
            StatusFilter::Running => "running",
            StatusFilter::Queued => "queued",
            StatusFilter::Blocked => "blocked",
        }
    }
}

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
    pub scheduler: Option<SchedulerType>,
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
    InternalScatterGather(Box<InternalScatterGatherArgs>),

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

    #[command(about = "View logs for a job")]
    Log(LogArgs),
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

    #[arg(
        long,
        value_enum,
        help = "Filter jobs by status. Can be repeated (e.g., --status failed --status blocked)"
    )]
    pub status: Vec<StatusFilter>,

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
    #[command(subcommand)]
    pub command: Option<GcCommand>,

    #[arg(
        long,
        global = true,
        help = "The target (must be defined in config.toml)"
    )]
    pub target: Option<String>,

    #[arg(
        long,
        global = true,
        help = "Preview what would be deleted without actually deleting anything"
    )]
    pub dry_run: bool,

    #[arg(long, short = 'y', global = true, help = "Skip confirmation prompt")]
    pub yes: bool,

    #[arg(
        long,
        help = "Remove all auto roots before collecting, keeping only explicitly pinned labs"
    )]
    pub pinned_only: bool,
}

#[derive(Subcommand)]
pub enum GcCommand {
    #[command(about = "List all GC roots (auto and pinned)")]
    List(GcListArgs),

    #[command(about = "Check if the current lab is pinned")]
    Status,

    #[command(about = "Pin a lab to prevent it from being garbage collected")]
    Pin(GcPinArgs),

    #[command(about = "Remove a pinned GC root")]
    Unpin(GcUnpinArgs),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ScatterGatherPhase {
    All,
    ScatterOnly,
    Step,
    Gather,
}

impl std::fmt::Display for ScatterGatherPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScatterGatherPhase::All => write!(f, "all"),
            ScatterGatherPhase::ScatterOnly => write!(f, "scatter-only"),
            ScatterGatherPhase::Step => write!(f, "step"),
            ScatterGatherPhase::Gather => write!(f, "gather"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum GcKindFilter {
    Auto,
    Pinned,
}

#[derive(Args)]
pub struct GcListArgs {
    #[arg(long, help = "Compute and display disk usage for each root")]
    pub sizes: bool,

    #[arg(long, value_enum, help = "Filter roots by kind")]
    pub kind: Option<GcKindFilter>,

    #[arg(long, help = "Filter auto roots by project ID (substring match)")]
    pub project: Option<String>,
}

#[derive(Args)]
pub struct GcPinArgs {
    #[arg()]
    pub lab_hash: Option<String>,

    #[arg(long)]
    pub name: Option<String>,
}

#[derive(Args)]
pub struct GcUnpinArgs {
    #[arg()]
    pub name: String,
}

#[derive(Args)]
pub struct LogArgs {
    #[arg(help = "Job ID (or prefix) to get logs for")]
    pub job_id: String,

    #[arg(
        short = 'n',
        long,
        default_value = "50",
        help = "Number of lines to show"
    )]
    pub lines: u32,

    #[arg(long, help = "Show stderr instead of stdout")]
    pub stderr: bool,

    #[arg(short, long, help = "Follow log output (like tail -f)")]
    pub follow: bool,
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

    #[arg(long, help = "Preview what would be deleted without actually deleting")]
    pub dry_run: bool,
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
        help = "Override the available memory for the local scheduler (e.g., 64G, 128G). By default, system RAM is detected automatically."
    )]
    pub mem: Option<String>,

    #[arg(
        long,
        help = "Continue running independent jobs even when some jobs fail. Report all failures at the end."
    )]
    pub continue_on_failure: bool,

    #[arg(long, help = "Disable wall clock timing on job completion messages.")]
    pub no_timing: bool,
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
    pub runtime: ExecutionType,
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

    #[arg(
        long,
        help = "Override the user output directory (for scatter-gather steps)."
    )]
    pub user_out_dir: Option<PathBuf>,
    #[arg(
        long,
        help = "Override the repx metadata directory (for scatter-gather steps)."
    )]
    pub repx_out_dir: Option<PathBuf>,
    #[arg(
        long,
        help = "Override the parameters JSON path (for scatter-gather steps)."
    )]
    pub parameters_json_path: Option<PathBuf>,
    #[arg(
        long,
        help = "Override the job package path (for scatter-gather steps)."
    )]
    pub job_package_path: Option<PathBuf>,
}

#[derive(Args)]
pub struct InternalScatterGatherArgs {
    #[arg(long, help = "The ID of the composite scatter-gather job.")]
    pub job_id: String,
    #[arg(long)]
    pub runtime: ExecutionType,
    #[arg(long)]
    pub image_tag: Option<String>,
    #[arg(long)]
    pub base_path: PathBuf,
    #[arg(long)]
    pub node_local_path: Option<PathBuf>,
    #[arg(long)]
    pub host_tools_dir: String,
    #[arg(long)]
    pub scheduler: SchedulerType,
    #[arg(long, allow_hyphen_values = true)]
    pub step_sbatch_opts: String,
    #[arg(long)]
    pub job_package_path: PathBuf,
    #[arg(long)]
    pub scatter_exe_path: PathBuf,
    #[arg(long)]
    pub gather_exe_path: PathBuf,
    #[arg(long)]
    pub steps_json: String,
    #[arg(long)]
    pub last_step_outputs_json: String,

    #[arg(long)]
    pub anchor_id: Option<u32>,

    #[arg(long, value_enum, default_value = "all")]
    pub phase: ScatterGatherPhase,

    #[arg(long)]
    pub branch_idx: Option<usize>,

    #[arg(long)]
    pub step_name: Option<String>,

    #[arg(long, default_value_t = false)]
    pub mount_host_paths: bool,
    #[arg(long)]
    pub mount_paths: Vec<String>,
}
