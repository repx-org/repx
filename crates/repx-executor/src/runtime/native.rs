use crate::error::Result;
use crate::ExecutionRequest;
use std::path::Path;
use tokio::process::Command as TokioCommand;

pub struct NativeRuntime;

impl NativeRuntime {
    pub fn build_command(
        request: &ExecutionRequest,
        script_path: &Path,
        args: &[String],
    ) -> Result<TokioCommand> {
        tracing::warn!(
            job_id = %request.job_id,
            script = %script_path.display(),
            "Executing job in native mode (no isolation). The script has full access to the \
             host filesystem and all system binaries. Use bwrap or container runtime for \
             sandboxed execution."
        );

        let mut rewritten_args: Vec<String> = args.to_vec();
        let mut _memfd_guards: Vec<std::os::fd::OwnedFd> = Vec::new();

        if let Some(ref data) = request.inputs_data {
            let (fd, owned) = super::bwrap::create_memfd_with_data(data, "repx-inputs")?;
            if rewritten_args.len() > 1 {
                rewritten_args[1] = format!("/proc/self/fd/{}", fd);
            }
            _memfd_guards.push(owned);
        }
        if let Some(ref data) = request.parameters_data {
            let (fd, owned) = super::bwrap::create_memfd_with_data(data, "repx-params")?;
            if rewritten_args.len() > 2 {
                rewritten_args[2] = format!("/proc/self/fd/{}", fd);
            }
            _memfd_guards.push(owned);
        }

        let mut cmd = TokioCommand::new(script_path);
        cmd.args(&rewritten_args);

        if let Some(host_tools) = &request.host_tools_bin_dir {
            if let Some(system_path) = std::env::var_os("PATH") {
                let mut paths = std::env::split_paths(&system_path).collect::<Vec<_>>();
                paths.insert(0, host_tools.clone());
                if let Ok(new_path) = std::env::join_paths(paths) {
                    cmd.env("PATH", new_path);
                }
            } else {
                cmd.env("PATH", host_tools);
            }
        }

        for guard in _memfd_guards {
            std::mem::forget(guard);
        }

        Ok(cmd)
    }
}
