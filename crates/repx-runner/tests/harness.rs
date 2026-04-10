#![allow(dead_code)]

use assert_cmd::Command as AssertCommand;
use repx_test_utils::harness::TestContext;
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

pub struct TestHarness {
    pub context: TestContext,
}

impl TestHarness {
    pub fn new() -> Self {
        Self {
            context: TestContext::new(),
        }
    }

    pub fn with_execution_type(exec_type: &str) -> Self {
        Self {
            context: TestContext::with_execution_type(exec_type),
        }
    }

    pub fn with_execution_type_and_lab(exec_type: &str, lab_env_var: &str) -> Self {
        Self {
            context: TestContext::with_execution_type_and_lab(exec_type, lab_env_var),
        }
    }

    #[allow(clippy::expect_used)]
    pub fn cmd(&self) -> AssertCommand {
        let mut cmd = AssertCommand::new(repx_binary_path());
        cmd.env("XDG_CONFIG_HOME", &self.context.config_dir);
        cmd.env("RUST_BACKTRACE", "1");
        cmd.arg("--lab").arg(&self.context.lab_path);
        cmd.env("REPX_TEST_LOG_TEE", "1");
        cmd.env("REPX_LOG_LEVEL", "DEBUG");
        cmd
    }
}

#[allow(clippy::expect_used)]
fn repx_binary_path() -> PathBuf {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY
        .get_or_init(|| {
            let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
            let status = Command::new(&cargo)
                .args(["build", "-p", "repx-cli", "--bin", "repx"])
                .status()
                .expect("failed to invoke `cargo build -p repx-cli`");
            assert!(status.success(), "`cargo build -p repx-cli` failed");

            let test_exe = std::env::current_exe().expect("current_exe");
            let target_profile = test_exe
                .parent()
                .and_then(|p| p.parent())
                .expect("test exe has no grandparent");
            let bin = target_profile.join(format!("repx{}", std::env::consts::EXE_SUFFIX));
            assert!(
                bin.is_file(),
                "repx binary missing after build: {}",
                bin.display()
            );
            bin
        })
        .clone()
}

impl Default for TestHarness {
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for TestHarness {
    type Target = TestContext;

    fn deref(&self) -> &Self::Target {
        &self.context
    }
}

impl DerefMut for TestHarness {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.context
    }
}
