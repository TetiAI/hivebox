//! Sandbox runtime: command execution and lifecycle management.
//!
//! This module handles running commands inside sandboxes and managing
//! their outputs. It provides both synchronous execution (wait for completion)
//! and the data structures needed for streaming output in later phases.

pub mod cleanup;
pub mod exec;

use serde::{Deserialize, Serialize};

/// Result of executing a command inside a sandbox.
///
/// Contains the captured stdout/stderr and exit code,
/// similar to what you'd get from `subprocess.run()` in Python.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecResult {
    /// Process exit code. 0 = success, non-zero = error.
    /// -1 indicates the process was killed by a signal.
    pub exit_code: i32,

    /// Captured standard output.
    pub stdout: String,

    /// Captured standard error.
    pub stderr: String,

    /// Wall-clock execution time in milliseconds.
    pub duration_ms: u64,

    /// Current working directory after command execution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,

    /// True if the process was killed by the OOM killer (out of memory).
    #[serde(default)]
    pub oom_killed: bool,
}

impl ExecResult {
    /// Returns true if the command exited successfully (code 0).
    pub fn success(&self) -> bool {
        self.exit_code == 0
    }
}
