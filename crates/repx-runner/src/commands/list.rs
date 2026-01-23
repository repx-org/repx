use repx_core::error::AppError;
use repx_core::lab;
use std::path::Path;

pub fn handle_list(lab_path: &Path) -> Result<(), AppError> {
    let lab = lab::load_from_path(lab_path)?;

    println!("Available runs in '{}':", lab_path.display());

    let mut run_ids: Vec<_> = lab.runs.keys().collect();
    run_ids.sort();

    for run_id in run_ids {
        println!("  {}", run_id);
    }

    Ok(())
}
