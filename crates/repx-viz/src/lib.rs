#[macro_use]
mod dot;
mod generator;
mod helpers;

use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use generator::VizGenerator;

#[derive(Debug, Clone)]
pub struct VizArgs {
    pub lab: PathBuf,
    pub output: Option<PathBuf>,
    pub format: Option<String>,

    pub show_pipelines: bool,
    pub show_runs: bool,
    pub show_groups: bool,

    pub show_params: bool,
    pub show_intra_edges: bool,
    pub show_inter_edges: bool,
}

pub fn run(args: VizArgs) -> Result<()> {
    if !args.show_pipelines && !args.show_runs && !args.show_groups {
        anyhow::bail!("Nothing to draw. Enable at least one of: --pipelines, --runs, --groups.");
    }

    let lab = repx_core::lab::load_from_path(&args.lab)?;

    let mut generator = VizGenerator::new(&lab);
    let dot_content = generator.generate_dot(&args)?;

    let output_base = args
        .output
        .clone()
        .unwrap_or_else(|| PathBuf::from("topology"));
    let format = args.format.unwrap_or_else(|| "png".to_string());

    let dot_path = if let Some(parent) = output_base.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
        output_base.with_extension("dot")
    } else {
        PathBuf::from("topology.dot")
    };

    fs::write(&dot_path, dot_content)?;

    println!("Rendering {}.{}...", output_base.display(), format);

    let output_file = output_base.with_extension(&format);

    let status = Command::new("dot")
        .arg(format!("-T{}", format))
        .arg(&dot_path)
        .arg("-o")
        .arg(&output_file)
        .status()
        .context("Failed to execute 'dot'. Is Graphviz installed?")?;

    if !status.success() {
        anyhow::bail!("Graphviz exited with error");
    }

    let _ = fs::remove_file(dot_path);

    println!("Done.");
    Ok(())
}
