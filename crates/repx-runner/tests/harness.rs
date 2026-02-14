#![allow(dead_code)]
use assert_cmd::Command as AssertCommand;
use repx_test_utils::harness::TestContext;
use std::ops::{Deref, DerefMut};

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

    pub fn cmd(&self) -> AssertCommand {
        let mut cmd = AssertCommand::new(env!("CARGO_BIN_EXE_repx-runner"));
        cmd.env("XDG_CONFIG_HOME", &self.context.config_dir);
        cmd.env("RUST_BACKTRACE", "1");
        cmd.arg("--lab").arg(&self.context.lab_path);
        cmd.env("REPX_TEST_LOG_TEE", "1");
        cmd.env("REPX_LOG_LEVEL", "DEBUG");
        cmd
    }
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
