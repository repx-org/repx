#![allow(dead_code)]

mod blueprint;
mod cartesian;
mod expand;
mod io;
mod metadata;
mod nix32;
#[cfg(test)]
mod tests;

use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Parser, Debug)]
#[command(version, about = "repx-expand: scalable job expansion engine")]
struct Args {
    #[arg(long)]
    blueprint: PathBuf,

    #[arg(long)]
    output: PathBuf,

    #[arg(long, default_value = "0.0.0")]
    lab_version: String,

    #[arg(long)]
    threads: Option<usize>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let start = Instant::now();

    if let Some(threads) = args.threads {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build_global()
            .context("configuring rayon thread pool")?;
    }

    let t0 = Instant::now();
    let blueprint_data =
        std::fs::read_to_string(&args.blueprint).context("reading blueprint JSON")?;
    let blueprint: blueprint::Blueprint =
        serde_json::from_str(&blueprint_data).context("parsing blueprint JSON")?;

    let num_runs = blueprint.runs.len();
    let total_combos: u128 = blueprint
        .runs
        .iter()
        .map(|r| {
            let (_axes, total) = cartesian::build_axes(r);
            let pipelines = r.pipelines.len() as u128;
            total * pipelines
        })
        .sum();

    eprintln!(
        "[repx-expand] Blueprint: {} runs, ~{} pipeline-combos ({:?})",
        num_runs,
        total_combos,
        t0.elapsed()
    );

    let t1 = Instant::now();
    let expanded = expand::expand_blueprint(blueprint);
    let total_jobs: usize = expanded.runs.iter().map(|r| r.jobs.len()).sum();
    eprintln!(
        "[repx-expand] Expanded {} jobs ({:?})",
        total_jobs,
        t1.elapsed()
    );

    let t2 = Instant::now();
    std::fs::create_dir_all(&args.output).context("creating output directory")?;

    let stats =
        io::assemble_lab(&expanded, &args.output, &args.lab_version).context("assembling lab")?;

    eprintln!(
        "[repx-expand] Assembled {} unique / {} total jobs, {} scripts ({:?})",
        stats.unique_jobs,
        stats.total_jobs,
        stats.script_copies,
        t2.elapsed()
    );

    eprintln!("[repx-expand] Total: {:?}", start.elapsed());

    Ok(())
}
