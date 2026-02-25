pub mod markers {
    pub const SUCCESS: &str = "SUCCESS";
    pub const FAIL: &str = "FAIL";
}

pub mod logs {
    pub const STDOUT: &str = "stdout.log";
    pub const STDERR: &str = "stderr.log";
}

pub mod manifests {
    pub const WORKER_SLURM_IDS: &str = "worker_slurm_ids.json";
}

pub mod dirs {
    pub const REPX: &str = "repx";
    pub const OUTPUTS: &str = "outputs";
    pub const ARTIFACTS: &str = "artifacts";
    pub const JOBS: &str = "jobs";
    pub const BIN: &str = "bin";
    pub const OUT: &str = "out";
}

pub mod targets {
    pub const LOCAL: &str = "local";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_marker_constants() {
        assert_eq!(markers::SUCCESS, "SUCCESS");
        assert_eq!(markers::FAIL, "FAIL");
    }

    #[test]
    fn test_log_constants() {
        assert_eq!(logs::STDOUT, "stdout.log");
        assert_eq!(logs::STDERR, "stderr.log");
    }

    #[test]
    fn test_dir_constants() {
        assert_eq!(dirs::REPX, "repx");
        assert_eq!(dirs::OUTPUTS, "outputs");
    }

    #[test]
    fn test_manifest_constants() {
        assert_eq!(manifests::WORKER_SLURM_IDS, "worker_slurm_ids.json");
    }
}
