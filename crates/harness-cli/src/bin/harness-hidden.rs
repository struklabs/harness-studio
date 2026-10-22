//! `harness-hidden`: runs `harness` (the file next to this one) with the same arguments and
//! no console window, then exits with its exit code.
//!
//! Windows only needs it: a console program that Task Scheduler starts at logon gets a
//! visible console window, and a GUI-subsystem program does not. `harness service install`
//! points the logon task here (`harness-hidden.exe serve ...`). The child runs with
//! `CREATE_NO_WINDOW` (a console without a window, so its console handlers still work) inside
//! a Job Object with `KILL_ON_JOB_CLOSE`: ending the task terminates this process and the job
//! takes `harness serve` (and its llama-servers) with it, as before.
//!
//! On other systems it simply runs `harness` and passes the exit code on.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::process::{Command, Stdio};

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    let Ok(me) = std::env::current_exe() else {
        return 1;
    };
    let harness = me.with_file_name(if cfg!(windows) {
        "harness.exe"
    } else {
        "harness"
    });
    let mut cmd = harness_core::process::hidden_command(harness);
    cmd.args(std::env::args_os().skip(1))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    spawn_and_wait(cmd)
}

#[cfg(windows)]
fn spawn_and_wait(mut cmd: Command) -> i32 {
    use command_group::CommandGroup;
    let mut group = cmd.group();
    group.kill_on_drop(true);
    harness_core::process::hide_window_group(&mut group);
    match group.spawn().and_then(|mut child| child.wait()) {
        Ok(status) => status.code().unwrap_or(1),
        Err(_) => 1,
    }
}

#[cfg(not(windows))]
fn spawn_and_wait(mut cmd: Command) -> i32 {
    match cmd.status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(_) => 1,
    }
}
