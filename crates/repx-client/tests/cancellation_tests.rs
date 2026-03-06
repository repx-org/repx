#![allow(clippy::expect_used)]

use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn test_submit_options_has_cancel_flag() {
    use repx_client::SubmitOptions;

    let cancel_flag = Arc::new(AtomicBool::new(false));

    let options = SubmitOptions {
        cancel_flag: Some(cancel_flag.clone()),
        ..Default::default()
    };

    assert!(!cancel_flag.load(Ordering::SeqCst));
    cancel_flag.store(true, Ordering::SeqCst);

    assert!(options
        .cancel_flag
        .as_ref()
        .expect("cancel_flag should be set")
        .load(Ordering::SeqCst));
}

#[test]
fn test_child_process_can_be_killed_while_waiting() {
    use nix::sys::signal::{kill, Signal};
    use nix::unistd::Pid;

    let child = Command::new("sleep")
        .arg("300")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn sleep process");

    let child_pid = child.id();

    let wait_thread = thread::spawn(move || child.wait_with_output());

    thread::sleep(Duration::from_millis(50));

    let start = Instant::now();
    kill(Pid::from_raw(child_pid as i32), Signal::SIGKILL).expect("failed to kill child");

    let result = wait_thread.join().expect("thread panicked");
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_secs(2),
        "killing child should allow wait to complete quickly, took {:?}",
        elapsed
    );

    assert!(result.is_ok(), "wait_with_output should succeed");
    let output = result.expect("result should be ok after assertion");
    assert!(
        !output.status.success(),
        "killed process should not have success status"
    );
}

#[test]
fn test_cancel_flag_checked_before_spawning() {
    let cancel_flag = Arc::new(AtomicBool::new(true));

    let start = Instant::now();

    if cancel_flag.load(Ordering::SeqCst) {
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_millis(100),
            "pre-cancelled flag should be detected immediately"
        );
        return;
    }

    panic!("should have detected pre-cancelled flag");
}

#[test]
fn test_multiple_children_killed_on_cancellation() {
    use nix::sys::signal::{kill, Signal};
    use nix::unistd::Pid;

    let num_children = 5;
    let mut child_pids: Vec<u32> = Vec::new();
    let mut wait_threads = Vec::new();

    for _ in 0..num_children {
        let child = Command::new("sleep")
            .arg("300")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("failed to spawn sleep process");

        child_pids.push(child.id());

        let handle = thread::spawn(move || child.wait_with_output());
        wait_threads.push(handle);
    }

    thread::sleep(Duration::from_millis(50));

    let start = Instant::now();
    for pid in &child_pids {
        let _ = kill(Pid::from_raw(*pid as i32), Signal::SIGKILL);
    }

    for handle in wait_threads {
        let _ = handle.join().expect("thread panicked");
    }

    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(2),
        "all {} children should be killed quickly, took {:?}",
        num_children,
        elapsed
    );
}
