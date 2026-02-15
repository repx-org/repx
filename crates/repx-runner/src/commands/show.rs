use crate::cli::{ShowArgs, ShowEntity, ShowJobArgs, ShowOutputArgs};
use crate::error::CliError;
use repx_core::{
    config::{self, Config},
    constants::{dirs, logs},
    errors::ConfigError,
    lab,
    model::{JobId, RunId},
    resolver,
    store::outcomes::{get_job_outcomes, JobOutcome},
};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

pub fn handle_show(args: ShowArgs, lab_path: &Path) -> Result<(), CliError> {
    match args.entity {
        ShowEntity::Job(job_args) => handle_show_job(job_args, lab_path),
        ShowEntity::Output(output_args) => handle_show_output(output_args, lab_path),
    }
}

fn handle_show_job(args: ShowJobArgs, lab_path: &Path) -> Result<(), CliError> {
    let lab = lab::load_from_path(lab_path)?;
    let config = config::load_config()?;

    let target_input = RunId(args.job_id.clone());
    let job_id = resolver::resolve_target_job_id(&lab, &target_input)?;

    let job = lab.jobs.get(job_id).ok_or_else(|| {
        CliError::Config(ConfigError::General(format!(
            "Job '{}' not found in lab",
            job_id
        )))
    })?;

    let run_name = lab
        .runs
        .iter()
        .find(|(_, run)| run.jobs.contains(job_id))
        .map(|(run_id, _)| run_id.0.clone());

    let store_path = get_store_path(&config)?;
    let outcomes = get_job_outcomes(&store_path, std::slice::from_ref(job_id))?;
    let status = outcomes.get(job_id).map(|found| match found.outcome {
        JobOutcome::Succeeded => "SUCCESS",
        JobOutcome::Failed => "FAILED",
    });

    println!("Job: {}", job_id.0);
    if let Some(name) = &job.name {
        println!("Name: {}", name);
    }
    if let Some(run) = &run_name {
        println!("Run: {}", run);
    }
    println!(
        "Status: {}",
        status.unwrap_or("PENDING (not executed or not found)")
    );
    println!("Stage Type: {}", job.stage_type);

    println!();
    println!("Parameters:");
    if job.params.is_null()
        || (job.params.is_object() && job.params.as_object().unwrap().is_empty())
    {
        println!("  (none)");
    } else {
        print_json_indented(&job.params, 2);
    }

    println!();
    println!("Inputs:");
    let mut inputs_found = false;
    let mut seen_deps: HashSet<&JobId> = HashSet::new();
    for (exe_name, exe) in &job.executables {
        for input in &exe.inputs {
            if let Some(dep_id) = &input.job_id {
                if seen_deps.insert(dep_id) {
                    inputs_found = true;
                    let short_id = dep_id.short_id();
                    println!("  {}: {} (from {})", input.target_input, short_id, exe_name);
                }
            }
        }
    }
    if !inputs_found {
        println!("  (none - this is a root job)");
    }

    println!();
    println!("Outputs:");
    let mut outputs_found = false;
    for (exe_name, exe) in &job.executables {
        if !exe.outputs.is_empty() {
            for (output_name, output_value) in &exe.outputs {
                outputs_found = true;
                if let Some(path) = output_value.as_str() {
                    println!("  {}: {} (from {})", output_name, path, exe_name);
                } else {
                    println!("  {}: {:?} (from {})", output_name, output_value, exe_name);
                }
            }
        }
    }
    if !outputs_found {
        println!("  (none declared)");
    }

    println!();
    println!("Paths:");
    let output_dir = store_path.join(dirs::OUTPUTS).join(&job_id.0);
    let out_dir = output_dir.join(dirs::OUT);
    let repx_dir = output_dir.join(dirs::REPX);

    if output_dir.exists() {
        println!("  output: {}", out_dir.display());
        println!("  logs:   {}", repx_dir.display());

        let stdout_log = repx_dir.join(logs::STDOUT);
        let stderr_log = repx_dir.join(logs::STDERR);
        if stdout_log.exists() || stderr_log.exists() {
            println!();
            println!("Log Files:");
            if stdout_log.exists() {
                println!("  stdout: {}", stdout_log.display());
            }
            if stderr_log.exists() {
                println!("  stderr: {}", stderr_log.display());
            }
        }

        if out_dir.exists() {
            println!();
            println!("Output Files:");
            list_directory_recursive(&out_dir, &out_dir, 2)?;
        }
    } else {
        println!("  (job not executed yet - no output directory)");
        println!("  expected: {}", output_dir.display());
    }

    Ok(())
}

fn get_store_path(config: &Config) -> Result<std::path::PathBuf, CliError> {
    let target_name = config.submission_target.as_ref().ok_or_else(|| {
        CliError::Config(ConfigError::General(
            "No submission target configured".to_string(),
        ))
    })?;

    let target = config.targets.get(target_name).ok_or_else(|| {
        CliError::Config(ConfigError::General(format!(
            "Target '{}' not found in config",
            target_name
        )))
    })?;

    Ok(target.base_path.clone())
}

fn print_json_indented(value: &serde_json::Value, indent: usize) {
    let prefix = " ".repeat(indent);
    match value {
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                match v {
                    serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
                        println!("{}{}:", prefix, k);
                        print_json_indented(v, indent + 2);
                    }
                    _ => {
                        println!("{}{}: {}", prefix, k, format_json_value(v));
                    }
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for (i, v) in arr.iter().enumerate() {
                println!("{}[{}]: {}", prefix, i, format_json_value(v));
            }
        }
        _ => {
            println!("{}{}", prefix, format_json_value(value));
        }
    }
}

fn format_json_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => "null".to_string(),
        _ => value.to_string(),
    }
}

fn list_directory_recursive(base: &Path, dir: &Path, indent: usize) -> Result<(), CliError> {
    let prefix = " ".repeat(indent);

    let mut entries: Vec<_> = fs::read_dir(dir)
        .map_err(|e| CliError::Config(ConfigError::Io(e)))?
        .filter_map(|e| e.ok())
        .collect();

    entries.sort_by_key(|e| e.path());

    for entry in entries {
        let path = entry.path();
        let relative = path.strip_prefix(base).unwrap_or(&path);

        if path.is_dir() {
            println!("{}{}/", prefix, relative.display());
        } else {
            let size = fs::metadata(&path)
                .map(|m| format_size(m.len()))
                .unwrap_or_else(|_| "?".to_string());
            println!("{}{} ({})", prefix, relative.display(), size);
        }
    }

    Ok(())
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1}G", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1}M", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1}K", bytes as f64 / KB as f64)
    } else {
        format!("{}B", bytes)
    }
}

fn handle_show_output(args: ShowOutputArgs, lab_path: &Path) -> Result<(), CliError> {
    let lab = lab::load_from_path(lab_path)?;
    let config = config::load_config()?;

    let target_input = RunId(args.job_id.clone());
    let job_id = resolver::resolve_target_job_id(&lab, &target_input)?;

    let store_path = get_store_path(&config)?;
    let output_dir = store_path.join(dirs::OUTPUTS).join(&job_id.0);
    let out_dir = output_dir.join(dirs::OUT);

    if !out_dir.exists() {
        return Err(CliError::Config(ConfigError::General(format!(
            "Output directory does not exist: {}\nJob may not have been executed yet.",
            out_dir.display()
        ))));
    }

    match args.path {
        Some(path) => {
            let file_path = out_dir.join(&path);
            if !file_path.exists() {
                eprintln!("File not found: {}", file_path.display());
                eprintln!();
                eprintln!("Available files in {}:", out_dir.display());
                list_directory_recursive(&out_dir, &out_dir, 2)?;
                return Err(CliError::Config(ConfigError::General(format!(
                    "File '{}' not found in job output",
                    path
                ))));
            }

            if file_path.is_dir() {
                println!("Contents of directory: {}", path);
                list_directory_recursive(&file_path, &file_path, 2)?;
            } else {
                let contents = fs::read_to_string(&file_path).map_err(|e| {
                    CliError::Config(ConfigError::General(format!(
                        "Failed to read file '{}': {}",
                        file_path.display(),
                        e
                    )))
                })?;
                print!("{}", contents);
            }
        }
        None => {
            println!("Output directory: {}", out_dir.display());
            println!();
            println!("Files:");
            list_directory_recursive(&out_dir, &out_dir, 2)?;
        }
    }

    Ok(())
}
