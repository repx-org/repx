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
    pub const GCROOTS: &str = "gcroots";
    pub const JOBS: &str = "jobs";
    pub const BIN: &str = "bin";
    pub const OUT: &str = "out";
}

pub mod targets {
    pub const LOCAL: &str = "local";
}
