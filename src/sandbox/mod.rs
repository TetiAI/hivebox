//! Core sandbox module — orchestrates the creation and execution of sandboxes.
//!
//! A sandbox is an isolated Linux environment created using kernel primitives:
//! namespaces for isolation, cgroups for resource limits, and pivot_root for
//! filesystem separation. From inside, it looks and behaves like a complete
//! Alpine Linux system. From outside, it's just a set of constrained processes.
//!
//! # Phase 1 flow (one-shot)
//!
//! ```text
//! create_and_run(config)
//!   ├── create output pipes (stdout/stderr)
//!   ├── spawn_in_namespace(child_fn)
//!   │   ├── [parent] write UID/GID maps
//!   │   ├── [parent] create cgroup + set limits + add child
//!   │   └── [child]  setup_rootfs → pivot_root → mount specials → exec command
//!   ├── [parent] collect output + wait for exit
//!   └── [parent] cleanup (cgroup + filesystem)
//! ```

pub mod capabilities;
pub mod cgroup;
pub mod filesystem;
pub mod landlock;
pub mod manager;
pub mod namespace;
pub mod network;
pub mod seccomp;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::runtime::cleanup::cleanup_sandbox;
use crate::runtime::exec::{
    collect_child_output, create_output_pipes, exec_command, setup_child_pipes,
};
use crate::runtime::ExecResult;

use self::capabilities::drop_capabilities;
use self::cgroup::{CgroupManager, ResourceLimits};
use self::filesystem::{
    do_pivot_root, mount_special_filesystems, prepare_rootfs, set_sandbox_hostname,
};
use self::landlock::apply_landlock_restrictions;
use self::network::NetworkMode;
use self::seccomp::{install_seccomp_filter, SeccompProfile};

/// Configuration for creating a sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    /// Optional user-assigned name. If not provided, a random ID is generated.
    /// Names must be unique across all active sandboxes.
    #[serde(default)]
    pub name: Option<String>,

    /// Rootfs image name (always "base" Alpine).
    pub image: String,

    /// Resource limits (memory, CPU, PIDs).
    pub limits: ResourceLimits,

    /// Network mode (none, isolated, shared:group).
    pub network: NetworkMode,

    /// Command to execute inside the sandbox.
    pub command: String,

    /// Which skills to enable for the opencode agent.
    /// - `None` (omitted): all default skills from the global skills directory.
    /// - `Some([])` (empty array): no skills at all.
    /// - `Some(["pdf", "docx"])`: only the listed skills.
    #[serde(default)]
    pub skills: Option<Vec<String>>,

    /// Additional MCP servers to register alongside the built-in hivebox MCP.
    /// Keys are server names; values follow opencode's MCP config format.
    /// Example: `{"my-server": {"type": "remote", "url": "...", "enabled": true}}`
    #[serde(default)]
    pub custom_mcps: Option<serde_json::Value>,

    /// Override the LLM base URL for this sandbox's opencode agent.
    #[serde(default)]
    pub llm_base_url: Option<String>,

    /// Override the LLM API key for this sandbox's opencode agent.
    #[serde(default)]
    pub llm_api_key: Option<String>,

    /// Override the LLM model for this sandbox's opencode agent.
    #[serde(default)]
    pub llm_model: Option<String>,

    /// Extra instructions appended to the default ones for this sandbox's opencode agent.
    #[serde(default)]
    pub instructions: Option<Vec<String>>,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            name: None,
            image: "base".to_string(),
            limits: ResourceLimits::default(),
            network: NetworkMode::None,
            command: String::new(),
            skills: None,
            custom_mcps: None,
            llm_base_url: None,
            llm_api_key: None,
            llm_model: None,
            instructions: None,
        }
    }
}

/// Current state of a sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SandboxState {
    /// Sandbox is being set up (namespaces created, filesystem mounting).
    Creating,
    /// Sandbox is actively running a command.
    Running,
    /// Sandbox has exited (process completed or was killed).
    Stopped,
    /// Sandbox resources have been cleaned up.
    Destroyed,
}

/// Generates a sandbox ID from an optional name, or creates a random one.
///
/// If a name is provided, it's used as-is (the caller should validate uniqueness).
/// Otherwise, generates "hb-" + 6 random hex characters (e.g., "hb-7f3a9b").
pub fn generate_sandbox_id() -> String {
    let uuid = Uuid::new_v4();
    let hex = uuid.as_simple().to_string();
    format!("hb-{}", &hex[..6])
}

/// Returns the provided name or generates a random sandbox ID.
pub fn resolve_sandbox_id(name: Option<&str>) -> String {
    match name {
        Some(n) if !n.is_empty() => n.to_string(),
        _ => generate_sandbox_id(),
    }
}

/// Creates a sandbox and runs a command in it (one-shot mode).
///
/// This is the main entry point for Phase 1. It:
/// 1. Prepares the rootfs (bind-mount for now, overlayfs in Phase 2)
/// 2. Creates output pipes for capturing stdout/stderr
/// 3. Spawns a child process in isolated namespaces
/// 4. The child sets up the filesystem, does pivot_root, and execs the command
/// 5. The parent collects output and waits for the child to exit
/// 6. Everything is cleaned up (cgroup, mounts, directories)
///
/// Returns the command's output (stdout, stderr, exit code).
pub fn create_and_run(config: &SandboxConfig) -> Result<ExecResult> {
    let sandbox_id = generate_sandbox_id();
    info!(
        sandbox = sandbox_id,
        image = config.image,
        command = config.command,
        "creating one-shot sandbox"
    );

    // Prepare the rootfs directory.
    let rootfs_path =
        prepare_rootfs(&sandbox_id, &config.image).context("failed to prepare rootfs")?;

    // Create pipes for capturing the child's stdout and stderr.
    let (stdout_pipe, stderr_pipe) =
        create_output_pipes().context("failed to create output pipes")?;

    // Save pipe read FDs before they're moved into the closure.
    let stdout_read = stdout_pipe.read_fd;
    let stderr_read = stderr_pipe.read_fd;
    let stdout_write = stdout_pipe.write_fd;
    let stderr_write = stderr_pipe.write_fd;

    // Clone the command for use inside the child closure.
    let command = config.command.clone();
    let sid = sandbox_id.clone();
    let rootfs = rootfs_path.clone();

    // Spawn the child process in fresh namespaces.
    // Everything inside this closure runs in the child (isolated) process.
    let child_pid = namespace::spawn_in_namespace(move || -> isize {
        // --- We are now inside the child process, in new namespaces ---

        // Redirect stdout/stderr to the pipes so the parent can capture output.
        let stdout_p = crate::runtime::exec::PipePair {
            read_fd: stdout_read,
            write_fd: stdout_write,
        };
        let stderr_p = crate::runtime::exec::PipePair {
            read_fd: stderr_read,
            write_fd: stderr_write,
        };
        if let Err(e) = setup_child_pipes(&stdout_p, &stderr_p) {
            eprintln!("hivebox: failed to setup pipes: {e}");
            return -1;
        }

        // Set up the sandbox filesystem and execute the command.
        if let Err(e) = child_setup_and_exec(&sid, &rootfs, &command) {
            // At this point stderr might be connected to the pipe,
            // so this error message reaches the parent.
            eprintln!("hivebox: sandbox setup failed: {e:#}");
            return -1;
        }

        // Unreachable — exec_command calls execvp which replaces this process.
        0
    })
    .context("failed to spawn sandbox process")?;

    // --- We are the parent process ---

    // Create the cgroup and apply resource limits.
    // This happens after clone but before the child starts doing real work
    // (the child is blocked on the sync pipe until UID/GID maps are set).
    let cgroup = CgroupManager::create(&sandbox_id).context("failed to create cgroup")?;

    cgroup
        .apply_limits(&config.limits)
        .context("failed to apply resource limits")?;

    cgroup
        .add_process(child_pid)
        .context("failed to add child to cgroup")?;

    // Set up networking for the sandbox (veth pairs, bridges, NAT).
    let net_info = network::setup_network(&sandbox_id, &config.network, child_pid)
        .context("failed to set up networking")?;

    // Collect the child's output and wait for it to exit.
    let stdout_pipe = crate::runtime::exec::PipePair {
        read_fd: stdout_read,
        write_fd: stdout_write,
    };
    let stderr_pipe = crate::runtime::exec::PipePair {
        read_fd: stderr_read,
        write_fd: stderr_write,
    };
    let result = collect_child_output(child_pid, stdout_pipe, stderr_pipe)
        .context("failed to collect child output")?;

    // Clean up networking resources (veth pairs, IP allocation).
    if let Err(e) = network::cleanup_network(&sandbox_id, &net_info) {
        warn!(sandbox = sandbox_id, error = %e, "network cleanup failed");
    }

    // Clean up everything: kill remaining processes, remove cgroup, unmount filesystem.
    if let Err(e) = cleanup_sandbox(&sandbox_id) {
        error!(
            sandbox = sandbox_id,
            error = %e,
            "cleanup failed (resources may leak)"
        );
    }

    info!(
        sandbox = sandbox_id,
        exit_code = result.exit_code,
        duration_ms = result.duration_ms,
        "sandbox completed"
    );

    Ok(result)
}

/// Child process setup: prepare filesystem, apply security, exec command.
///
/// This runs inside the child process after namespace creation and UID/GID mapping.
/// On success, `exec_command` replaces this process with the user's command
/// and this function never returns.
///
/// # Security application order
///
/// 1. Mount filesystems (needs capabilities)
/// 2. `pivot_root` (isolates filesystem)
/// 3. Mount special filesystems (/proc, /dev, etc.)
/// 4. **Apply Landlock** (restricts filesystem access paths)
/// 5. **Drop capabilities** + set NO_NEW_PRIVS
/// 6. **Install seccomp filter** (must be last — requires NO_NEW_PRIVS)
/// 7. `execve` (run the user's command)
fn child_setup_and_exec(sandbox_id: &str, rootfs: &std::path::Path, command: &str) -> Result<()> {
    // Step 1: Set the sandbox's hostname (visible only inside the UTS namespace).
    set_sandbox_hostname(sandbox_id)?;

    // Step 2: pivot_root — make the sandbox rootfs the new `/`.
    // After this, the host filesystem is completely unreachable.
    do_pivot_root(rootfs)?;

    // Step 3: Mount /proc, /sys, /dev, /tmp inside the sandbox.
    mount_special_filesystems()?;

    // Step 4: Apply Landlock filesystem restrictions.
    // This limits which paths can be accessed, even for allowed syscalls.
    // Best-effort: on older kernels without Landlock, this is a no-op.
    if let Err(e) = apply_landlock_restrictions() {
        // Landlock failure is non-fatal — the sandbox still has namespace isolation.
        eprintln!("hivebox: landlock setup failed (non-fatal): {e:#}");
    }

    // Step 5: Drop capabilities and set NO_NEW_PRIVS.
    // This must happen after all mount operations (which need caps)
    // but before seccomp (which needs NO_NEW_PRIVS).
    if let Err(e) = drop_capabilities() {
        eprintln!("hivebox: capability drop failed (non-fatal): {e:#}");
    }

    // Step 6: Install seccomp-BPF filter (must be last security step).
    // After this, only whitelisted syscalls are allowed.
    if let Err(e) = install_seccomp_filter(SeccompProfile::Default) {
        eprintln!("hivebox: seccomp filter failed (non-fatal): {e:#}");
    }

    // Step 7: Execute the user's command. This calls execvp and never returns.
    exec_command(command)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_sandbox_id_format() {
        let id = generate_sandbox_id();
        assert!(id.starts_with("hb-"));
        assert_eq!(id.len(), 9); // "hb-" (3) + 6 hex chars
    }

    #[test]
    fn test_generate_sandbox_id_unique() {
        let id1 = generate_sandbox_id();
        let id2 = generate_sandbox_id();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_sandbox_config_default() {
        let config = SandboxConfig::default();
        assert_eq!(config.image, "base");
        assert!(config.command.is_empty());
    }
}
