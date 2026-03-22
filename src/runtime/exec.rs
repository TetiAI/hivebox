//! Command execution inside sandboxes.
//!
//! Handles running a command (via `execvp`) inside the sandbox's isolated namespaces,
//! capturing stdout/stderr through pipes, and collecting the exit status.
//!
//! # Pipe architecture
//!
//! ```text
//! Parent process              Child process (in sandbox)
//! ─────────────              ──────────────────────────
//! creates pipes  ──────────>  stdout_write → dup2 to fd 1
//!                             stderr_write → dup2 to fd 2
//! reads stdout_read  <──────  (writes to stdout go through pipe)
//! reads stderr_read  <──────  (writes to stderr go through pipe)
//! waitpid()          <──────  execvp(command) → exit
//! ```

use std::ffi::CString;
use std::io::Read;
use std::os::unix::io::{FromRawFd, IntoRawFd, RawFd};
use std::time::Instant;

use anyhow::{Context, Result};
use nix::sys::wait::{waitpid, WaitStatus};
use nix::unistd::{dup2, execvp, Pid};
use tracing::{debug, error, info};

use super::ExecResult;

/// Pipe pair: (read_end, write_end) as raw file descriptors.
pub struct PipePair {
    pub read_fd: RawFd,
    pub write_fd: RawFd,
}

impl PipePair {
    /// Creates a new pipe pair.
    ///
    /// Converts nix's OwnedFd to raw FDs for manual lifetime management
    /// across fork/clone boundaries.
    pub fn new() -> Result<Self> {
        let (read_fd, write_fd) = nix::unistd::pipe().context("failed to create pipe")?;
        // Convert OwnedFd to raw FDs — we manage lifetimes manually.
        Ok(Self {
            read_fd: read_fd.into_raw_fd(),
            write_fd: write_fd.into_raw_fd(),
        })
    }
}

/// Creates stdout and stderr pipes for capturing child output.
///
/// Returns (stdout_pipe, stderr_pipe). The parent reads from the read ends;
/// the child writes to the write ends (via dup2 to fd 1 and fd 2).
pub fn create_output_pipes() -> Result<(PipePair, PipePair)> {
    let stdout_pipe = PipePair::new().context("failed to create stdout pipe")?;
    let stderr_pipe = PipePair::new().context("failed to create stderr pipe")?;
    Ok((stdout_pipe, stderr_pipe))
}

/// Child-side setup: redirect stdout/stderr to the pipe write ends.
///
/// Called inside the child process after fork/clone, before execvp.
/// Closes the read ends (parent's responsibility) and dups the write ends
/// to file descriptors 1 (stdout) and 2 (stderr).
pub fn setup_child_pipes(stdout_pipe: &PipePair, stderr_pipe: &PipePair) -> Result<()> {
    // Close read ends — the child doesn't read from the pipes.
    unsafe { libc::close(stdout_pipe.read_fd) };
    unsafe { libc::close(stderr_pipe.read_fd) };

    // Redirect stdout (fd 1) to the stdout pipe write end.
    dup2(stdout_pipe.write_fd, 1).context("failed to dup2 stdout")?;

    // Redirect stderr (fd 2) to the stderr pipe write end.
    dup2(stderr_pipe.write_fd, 2).context("failed to dup2 stderr")?;

    // Close the original write fds (they're now duplicated to fd 1 and 2).
    unsafe { libc::close(stdout_pipe.write_fd) };
    unsafe { libc::close(stderr_pipe.write_fd) };

    Ok(())
}

/// Executes a shell command via `/bin/sh -c`.
///
/// This is called inside the child process as the final step — `execvp` replaces
/// the current process image with the shell executing the user's command.
///
/// # Never returns on success
///
/// `execvp` replaces the process, so this function only returns on error.
pub fn exec_command(command: &str) -> Result<()> {
    debug!(command, "executing command in sandbox");

    let sh = CString::new("/bin/sh").unwrap();
    let flag_c = CString::new("-c").unwrap();
    let cmd = CString::new(command).context("command contains null bytes")?;

    // execvp replaces the current process with /bin/sh -c "<command>".
    // On success, this never returns. On failure, we get an error.
    execvp(&sh, &[&sh, &flag_c, &cmd])
        .context("execvp failed — is /bin/sh available in the rootfs?")?;

    // Unreachable if execvp succeeds.
    unreachable!("execvp returned successfully, which should not happen");
}

/// Parent-side: reads child output from pipes and waits for exit.
///
/// Closes the write ends of the pipes (child's responsibility), reads all data
/// from the read ends, then calls `waitpid` to collect the exit status.
pub fn collect_child_output(
    child_pid: Pid,
    stdout_pipe: PipePair,
    stderr_pipe: PipePair,
) -> Result<ExecResult> {
    let start = Instant::now();

    // Close write ends — only the child writes to these.
    unsafe { libc::close(stdout_pipe.write_fd) };
    unsafe { libc::close(stderr_pipe.write_fd) };

    // Read all stdout from the pipe.
    let stdout = read_pipe_to_string(stdout_pipe.read_fd).context("failed to read child stdout")?;

    // Read all stderr from the pipe.
    let stderr = read_pipe_to_string(stderr_pipe.read_fd).context("failed to read child stderr")?;

    // Wait for the child process to exit and collect its status.
    let exit_code = match waitpid(child_pid, None) {
        Ok(WaitStatus::Exited(_, code)) => {
            debug!(pid = child_pid.as_raw(), code, "child exited normally");
            code
        }
        Ok(WaitStatus::Signaled(_, signal, _)) => {
            info!(
                pid = child_pid.as_raw(),
                signal = signal.as_str(),
                "child killed by signal"
            );
            // Convention: exit code = 128 + signal number when killed by signal.
            128 + signal as i32
        }
        Ok(status) => {
            error!(pid = child_pid.as_raw(), ?status, "unexpected wait status");
            -1
        }
        Err(e) => {
            error!(pid = child_pid.as_raw(), error = %e, "waitpid failed");
            -1
        }
    };

    let duration_ms = start.elapsed().as_millis() as u64;

    Ok(ExecResult {
        exit_code,
        stdout,
        stderr,
        duration_ms,
        cwd: None,
        oom_killed: false,
    })
}

/// Reads all data from a file descriptor into a String.
///
/// Consumes and closes the file descriptor.
fn read_pipe_to_string(fd: RawFd) -> Result<String> {
    let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
    let mut output = String::new();
    file.read_to_string(&mut output)
        .context("failed to read from pipe")?;
    // File is dropped here, which closes the fd.
    Ok(output)
}
