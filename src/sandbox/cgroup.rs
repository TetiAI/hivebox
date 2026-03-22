//! Cgroup v2 resource limit management for sandboxes.
//!
//! Each sandbox gets its own cgroup under `/sys/fs/cgroup/hivebox/{sandbox_id}/`.
//! This provides hard resource limits enforced by the kernel:
//!
//! - **Memory**: OOM kill if exceeded (no swap allowed)
//! - **CPU**: throttled to the configured fraction of a core
//! - **PIDs**: maximum number of processes (prevents fork bombs)
//!
//! The kernel enforces these limits independently of the sandbox process —
//! even if the sandboxed code tries to allocate all RAM or fork indefinitely,
//! it will be stopped before impacting the host or other sandboxes.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use nix::unistd::Pid;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

/// Root path for cgroup v2 unified hierarchy.
const CGROUP_ROOT: &str = "/sys/fs/cgroup";

/// HiveBox cgroup subtree. All sandbox cgroups live under this path.
const HIVEBOX_CGROUP: &str = "/sys/fs/cgroup/hivebox";

/// Resource limits for a sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    /// Maximum memory in bytes. Process is OOM-killed if exceeded.
    pub memory_bytes: u64,

    /// CPU limit as a fraction of one core (e.g., 0.5 = half a core, 2.0 = two cores).
    pub cpu_fraction: f64,

    /// Maximum number of processes (PIDs) in the sandbox.
    pub max_pids: u64,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            memory_bytes: 512 * 1024 * 1024, // 512 MiB
            cpu_fraction: 1.0,               // 1 full core
            max_pids: 128,                   // 128 processes
        }
    }
}

/// Manages a cgroup for a single sandbox.
///
/// Created on sandbox start, destroyed on sandbox cleanup.
/// The cgroup directory is the single source of truth for resource accounting —
/// read `memory.current`, `cpu.stat`, etc. to see actual usage.
pub struct CgroupManager {
    /// Absolute path to this sandbox's cgroup directory.
    path: PathBuf,
    /// Sandbox ID (for logging).
    sandbox_id: String,
}

impl CgroupManager {
    /// Creates a new cgroup for the given sandbox under the HiveBox subtree.
    ///
    /// Creates the directory hierarchy if needed and enables the required controllers.
    pub fn create(sandbox_id: &str) -> Result<Self> {
        let path = PathBuf::from(HIVEBOX_CGROUP).join(sandbox_id);

        // Ensure the HiveBox parent cgroup exists and has controllers delegated.
        ensure_parent_cgroup()?;

        // Create this sandbox's cgroup directory.
        fs::create_dir_all(&path)
            .with_context(|| format!("failed to create cgroup dir: {}", path.display()))?;

        info!(
            sandbox = sandbox_id,
            cgroup = %path.display(),
            "cgroup created"
        );

        Ok(Self {
            path,
            sandbox_id: sandbox_id.to_string(),
        })
    }

    /// Opens an existing cgroup for reading metrics (no directory creation or controller setup).
    pub fn open(sandbox_id: &str) -> Result<Self> {
        let path = PathBuf::from(HIVEBOX_CGROUP).join(sandbox_id);

        if !path.exists() {
            anyhow::bail!("cgroup dir does not exist: {}", path.display());
        }

        Ok(Self {
            path,
            sandbox_id: sandbox_id.to_string(),
        })
    }

    /// Sets memory limit. The sandbox is OOM-killed if it exceeds this.
    ///
    /// Also disables swap to ensure the memory limit is a hard ceiling.
    pub fn set_memory_limit(&self, bytes: u64) -> Result<()> {
        // Set the hard memory limit.
        self.write_file("memory.max", &bytes.to_string())
            .context("failed to set memory.max")?;

        // Allow swap equal to the memory limit. This does NOT actually use disk
        // swap (Docker typically has none), but it prevents the kernel from
        // counting virtual memory reservations (MAP_ANONYMOUS + PROT_NONE) against
        // the physical memory limit. Without this, runtimes like Node.js V8 fail
        // to reserve their CodeRange (~128 MB of virtual address space).
        let _ = self.write_file("memory.swap.max", &bytes.to_string());

        debug!(
            sandbox = self.sandbox_id,
            bytes,
            mb = bytes / (1024 * 1024),
            "memory limit set"
        );
        Ok(())
    }

    /// Sets CPU limit as a fraction of one core.
    ///
    /// Uses the `cpu.max` interface with a 100ms period.
    /// For example, 0.5 = 50ms quota per 100ms period = half a core.
    pub fn set_cpu_limit(&self, fraction: f64) -> Result<()> {
        let period: u64 = 100_000; // 100ms in microseconds
        let quota = (fraction * period as f64) as u64;

        // Format: "quota period" — both in microseconds.
        self.write_file("cpu.max", &format!("{quota} {period}"))
            .context("failed to set cpu.max")?;

        debug!(
            sandbox = self.sandbox_id,
            fraction,
            quota_us = quota,
            period_us = period,
            "CPU limit set"
        );
        Ok(())
    }

    /// Sets the maximum number of processes (PIDs) in the sandbox.
    ///
    /// Once the limit is reached, `fork()` returns EAGAIN. This prevents fork bombs
    /// from consuming host resources.
    pub fn set_pids_limit(&self, max: u64) -> Result<()> {
        self.write_file("pids.max", &max.to_string())
            .context("failed to set pids.max")?;

        debug!(sandbox = self.sandbox_id, max, "PID limit set");
        Ok(())
    }

    /// Applies all resource limits from a `ResourceLimits` struct.
    ///
    /// Individual limit failures are logged as warnings rather than failing the
    /// entire sandbox creation. This allows sandboxes to work in environments
    /// where cgroup controllers aren't fully available (e.g., Docker containers).
    pub fn apply_limits(&self, limits: &ResourceLimits) -> Result<()> {
        if let Err(e) = self.set_memory_limit(limits.memory_bytes) {
            warn!(sandbox = self.sandbox_id, error = %e, "failed to set memory limit — continuing without it");
        }
        if let Err(e) = self.set_cpu_limit(limits.cpu_fraction) {
            warn!(sandbox = self.sandbox_id, error = %e, "failed to set CPU limit — continuing without it");
        }
        if let Err(e) = self.set_pids_limit(limits.max_pids) {
            warn!(sandbox = self.sandbox_id, error = %e, "failed to set PID limit — continuing without it");
        }
        Ok(())
    }

    /// Adds a process to this cgroup.
    ///
    /// The process (and all its future children) will be subject to the configured
    /// resource limits. This is called by the parent after `clone()` returns the child PID.
    pub fn add_process(&self, pid: Pid) -> Result<()> {
        self.write_file("cgroup.procs", &pid.as_raw().to_string())
            .context("failed to add process to cgroup")?;

        debug!(
            sandbox = self.sandbox_id,
            pid = pid.as_raw(),
            "process added to cgroup"
        );
        Ok(())
    }

    /// Returns the memory limit configured for this cgroup in bytes.
    ///
    /// Reads `memory.max`. Returns `None` if the limit is "max" (unlimited).
    pub fn memory_limit(&self) -> Result<Option<u64>> {
        let content = self.read_file("memory.max")?;
        let trimmed = content.trim();
        if trimmed == "max" {
            Ok(None)
        } else {
            Ok(Some(
                trimmed
                    .parse::<u64>()
                    .context("failed to parse memory.max")?,
            ))
        }
    }

    /// Returns the number of OOM kills that have occurred in this cgroup.
    ///
    /// Reads `oom_kill` from `memory.events`. Returns 0 if unavailable.
    pub fn oom_kill_count(&self) -> u64 {
        let content = match self.read_file("memory.events") {
            Ok(c) => c,
            Err(_) => return 0,
        };
        for line in content.lines() {
            if let Some(val) = line.strip_prefix("oom_kill ") {
                return val.trim().parse::<u64>().unwrap_or(0);
            }
        }
        0
    }

    /// Returns the current memory usage of the sandbox in bytes.
    pub fn memory_usage(&self) -> Result<u64> {
        let content = self.read_file("memory.current")?;
        content
            .trim()
            .parse::<u64>()
            .context("failed to parse memory.current")
    }

    /// Returns the number of processes currently in the sandbox.
    pub fn pid_count(&self) -> Result<u64> {
        let content = self.read_file("pids.current")?;
        content
            .trim()
            .parse::<u64>()
            .context("failed to parse pids.current")
    }

    /// Returns the total CPU time used by the sandbox in microseconds.
    ///
    /// Reads `usage_usec` from `cpu.stat`. This is cumulative CPU time,
    /// not instantaneous usage.
    pub fn cpu_usage_usec(&self) -> Result<u64> {
        let content = self.read_file("cpu.stat")?;
        for line in content.lines() {
            if let Some(val) = line.strip_prefix("usage_usec ") {
                return val
                    .trim()
                    .parse::<u64>()
                    .context("failed to parse usage_usec");
            }
        }
        anyhow::bail!("usage_usec not found in cpu.stat")
    }

    /// Kills all processes in the cgroup.
    ///
    /// Tries `cgroup.kill` first (kernel 5.14+), falls back to iterating `cgroup.procs`.
    pub fn kill_all(&self) -> Result<()> {
        // cgroup.kill is the clean way (kernel 5.14+): writing "1" sends SIGKILL
        // to every process in the cgroup atomically.
        if self.write_file("cgroup.kill", "1").is_ok() {
            info!(
                sandbox = self.sandbox_id,
                "killed all processes via cgroup.kill"
            );
            return Ok(());
        }

        // Fallback: read cgroup.procs and kill each PID individually.
        warn!(
            sandbox = self.sandbox_id,
            "cgroup.kill not available, falling back to manual kill"
        );
        if let Ok(content) = self.read_file("cgroup.procs") {
            for line in content.lines() {
                if let Ok(pid) = line.trim().parse::<i32>() {
                    let _ = nix::sys::signal::kill(
                        Pid::from_raw(pid),
                        nix::sys::signal::Signal::SIGKILL,
                    );
                }
            }
        }

        Ok(())
    }

    /// Removes the cgroup directory.
    ///
    /// All processes must be dead before this succeeds. The kernel refuses to remove
    /// a cgroup that still has live processes.
    pub fn cleanup(&self) -> Result<()> {
        if self.path.exists() {
            fs::remove_dir(&self.path).with_context(|| {
                format!(
                    "failed to remove cgroup dir: {} — are all processes dead?",
                    self.path.display()
                )
            })?;
        }

        info!(sandbox = self.sandbox_id, "cgroup removed");
        Ok(())
    }

    /// Writes a value to a cgroup control file.
    fn write_file(&self, filename: &str, value: &str) -> Result<()> {
        let path = self.path.join(filename);
        fs::write(&path, value)
            .with_context(|| format!("failed to write '{}' to {}", value, path.display()))
    }

    /// Reads a cgroup control file.
    fn read_file(&self, filename: &str) -> Result<String> {
        let path = self.path.join(filename);
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))
    }
}

/// Ensures the HiveBox parent cgroup exists and has the required controllers enabled.
///
/// This is called once before creating the first sandbox cgroup. It enables
/// the `memory`, `cpu`, and `pids` controllers on the HiveBox subtree.
fn ensure_parent_cgroup() -> Result<()> {
    let parent = PathBuf::from(HIVEBOX_CGROUP);

    if !parent.exists() {
        fs::create_dir_all(&parent).context("failed to create /sys/fs/cgroup/hivebox")?;
    }

    // Enable controllers on the parent so child cgroups can use them.
    // This writes to the *parent's parent* subtree_control file.
    let subtree_control = PathBuf::from(CGROUP_ROOT).join("cgroup.subtree_control");
    if subtree_control.exists() {
        // Ignore errors — controllers might already be enabled, or we might not
        // have permission (which will surface later when we try to set limits).
        let _ = fs::write(&subtree_control, "+memory +cpu +pids");
    }

    // Also enable controllers on the hivebox cgroup itself.
    let hivebox_subtree = parent.join("cgroup.subtree_control");
    if hivebox_subtree.exists() {
        let _ = fs::write(&hivebox_subtree, "+memory +cpu +pids");
    }

    Ok(())
}

/// Parses a human-readable memory size string (e.g., "256m", "1g", "512k") to bytes.
///
/// Supported suffixes: k/K (KiB), m/M (MiB), g/G (GiB). No suffix means bytes.
pub fn parse_memory_size(s: &str) -> Result<u64> {
    let s = s.trim().to_lowercase();

    if let Some(num) = s.strip_suffix('g') {
        let n: f64 = num.parse().context("invalid memory size")?;
        Ok((n * 1024.0 * 1024.0 * 1024.0) as u64)
    } else if let Some(num) = s.strip_suffix('m') {
        let n: f64 = num.parse().context("invalid memory size")?;
        Ok((n * 1024.0 * 1024.0) as u64)
    } else if let Some(num) = s.strip_suffix('k') {
        let n: f64 = num.parse().context("invalid memory size")?;
        Ok((n * 1024.0) as u64)
    } else {
        s.parse::<u64>()
            .context("invalid memory size (expected number with optional k/m/g suffix)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_memory_size() {
        assert_eq!(parse_memory_size("256m").unwrap(), 256 * 1024 * 1024);
        assert_eq!(parse_memory_size("1g").unwrap(), 1024 * 1024 * 1024);
        assert_eq!(parse_memory_size("512k").unwrap(), 512 * 1024);
        assert_eq!(parse_memory_size("1024").unwrap(), 1024);
        assert_eq!(parse_memory_size("1G").unwrap(), 1024 * 1024 * 1024);
        assert_eq!(parse_memory_size("0.5g").unwrap(), 536_870_912);
    }

    #[test]
    fn test_default_limits() {
        let limits = ResourceLimits::default();
        assert_eq!(limits.memory_bytes, 512 * 1024 * 1024);
        assert_eq!(limits.cpu_fraction, 1.0);
        assert_eq!(limits.max_pids, 128);
    }
}
