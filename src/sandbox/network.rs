//! Network namespace configuration for sandboxes.
//!
//! Each sandbox gets its own network namespace (via `CLONE_NEWNET`). By default,
//! this namespace is empty — not even a loopback interface. This module configures
//! networking inside the sandbox based on the requested mode:
//!
//! - **None**: No network at all (not even loopback). Maximum isolation.
//! - **Isolated**: veth pair with NAT to the internet. Can reach external services
//!   but cannot communicate with the host or other sandboxes.
//! - **Shared**: veth pair on a named bridge, allowing selected sandboxes to
//!   communicate with each other on a private network.
//!
//! # Implementation
//!
//! Networking is configured by shelling out to `ip` and `iptables`. This is pragmatic
//! and debuggable — you can inspect the state with standard networking tools.
//! A future optimization could use the `rtnetlink` crate for pure-Rust netlink
//! socket communication.
//!
//! # Network topology
//!
//! ```text
//! Isolated mode:
//!
//!  Host                         Sandbox
//!  ┌────────────────┐          ┌──────────────────┐
//!  │ veth-{id}      │──────────│ eth0 (10.10.0.x) │
//!  │ (on hivebox0   │          │                    │
//!  │  bridge)       │          │ lo (127.0.0.1)    │
//!  └────────────────┘          └──────────────────┘
//!         │
//!    hivebox0 bridge (10.10.0.1)
//!         │
//!    iptables MASQUERADE ───── internet
//! ```

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use nix::unistd::Pid;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

/// Default bridge name for isolated-mode sandboxes.
const DEFAULT_BRIDGE: &str = "hivebox0";

/// Default subnet for the bridge (10.10.0.0/16).
const DEFAULT_SUBNET: &str = "10.10.0.0/16";

/// Bridge gateway IP (first usable address in the subnet).
const DEFAULT_GATEWAY: &str = "10.10.0.1";

/// IP allocator state file.
const IP_STATE_FILE: &str = "/var/lib/hivebox/network/ip_state.json";

/// Network mode for a sandbox.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum NetworkMode {
    /// No network at all (not even loopback). Maximum isolation.
    #[default]
    None,

    /// NAT to the internet via a veth pair on the default bridge.
    /// Can reach external services but not the host or other sandboxes.
    Isolated,

    /// Shared bridge with other sandboxes in the same group.
    /// Sandboxes in the same group can communicate with each other.
    Shared { group: String },
}

impl std::fmt::Display for NetworkMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => write!(f, "none"),
            Self::Isolated => write!(f, "isolated"),
            Self::Shared { group } => write!(f, "shared:{group}"),
        }
    }
}

/// Parses a network mode string from CLI input.
///
/// Formats: "none", "isolated", "shared:groupname"
pub fn parse_network_mode(s: &str) -> Result<NetworkMode> {
    match s {
        "none" => Ok(NetworkMode::None),
        "isolated" => Ok(NetworkMode::Isolated),
        s if s.starts_with("shared:") => {
            let group = s.strip_prefix("shared:").unwrap().to_string();
            if group.is_empty() {
                bail!("shared network mode requires a group name (e.g., shared:mygroup)");
            }
            Ok(NetworkMode::Shared { group })
        }
        _ => bail!(
            "invalid network mode '{}' — use none, isolated, or shared:groupname",
            s
        ),
    }
}

/// Network information for a running sandbox.
#[derive(Debug, Clone, Serialize)]
pub struct NetworkInfo {
    /// Network mode.
    pub mode: NetworkMode,

    /// IP address assigned to the sandbox (if any).
    pub ip: Option<String>,

    /// Bridge name the sandbox is connected to (if any).
    pub bridge: Option<String>,

    /// Host-side veth interface name (if any).
    pub veth_host: Option<String>,
}

/// Sets up networking for a sandbox based on the requested mode.
///
/// Must be called from the parent process after `clone()` returns the child PID,
/// because we need to move the veth peer into the child's network namespace.
pub fn setup_network(sandbox_id: &str, mode: &NetworkMode, child_pid: Pid) -> Result<NetworkInfo> {
    match mode {
        NetworkMode::None => {
            // Bring up loopback inside the namespace.
            // Even in "none" mode, some programs expect localhost to work.
            bring_up_loopback(child_pid)?;

            Ok(NetworkInfo {
                mode: mode.clone(),
                ip: None,
                bridge: None,
                veth_host: None,
            })
        }

        NetworkMode::Isolated => {
            let bridge = DEFAULT_BRIDGE.to_string();
            ensure_bridge(&bridge, DEFAULT_GATEWAY)?;

            let ip = allocate_ip(sandbox_id)?;
            let veth_host = format!("veth-{}", &sandbox_id[..sandbox_id.len().min(6)]);
            let veth_sandbox = "eth0";

            setup_veth_pair(
                sandbox_id,
                &veth_host,
                veth_sandbox,
                &bridge,
                &ip,
                DEFAULT_GATEWAY,
                child_pid,
            )?;

            Ok(NetworkInfo {
                mode: mode.clone(),
                ip: Some(ip),
                bridge: Some(bridge),
                veth_host: Some(veth_host),
            })
        }

        NetworkMode::Shared { group } => {
            let bridge = format!("hb-{}", &group[..group.len().min(10)]);
            let gateway = "10.20.0.1"; // Different subnet for shared bridges.
            ensure_bridge(&bridge, gateway)?;

            let ip = allocate_ip(sandbox_id)?;
            let veth_host = format!("veth-{}", &sandbox_id[..sandbox_id.len().min(6)]);
            let veth_sandbox = "eth0";

            setup_veth_pair(
                sandbox_id,
                &veth_host,
                veth_sandbox,
                &bridge,
                &ip,
                gateway,
                child_pid,
            )?;

            Ok(NetworkInfo {
                mode: mode.clone(),
                ip: Some(ip),
                bridge: Some(bridge),
                veth_host: Some(veth_host),
            })
        }
    }
}

/// Brings up the loopback interface inside a network namespace.
fn bring_up_loopback(pid: Pid) -> Result<()> {
    run_cmd(
        "ip",
        &[
            "netns",
            "exec",
            &format!("/proc/{}/ns/net", pid.as_raw()),
            "ip",
            "link",
            "set",
            "lo",
            "up",
        ],
    )
    // nsenter is more reliable than `ip netns exec` with a proc path.
    .or_else(|_| {
        run_cmd(
            "nsenter",
            &[
                &format!("--net=/proc/{}/ns/net", pid.as_raw()),
                "ip",
                "link",
                "set",
                "lo",
                "up",
            ],
        )
    })
    .context("failed to bring up loopback in sandbox")?;

    debug!(pid = pid.as_raw(), "loopback brought up");
    Ok(())
}

/// Ensures a bridge interface exists and is configured.
fn ensure_bridge(name: &str, gateway: &str) -> Result<()> {
    // Check if bridge already exists.
    if run_cmd("ip", &["link", "show", name]).is_ok() {
        debug!(bridge = name, "bridge already exists");
        return Ok(());
    }

    info!(bridge = name, gateway, "creating bridge");

    // Create the bridge.
    run_cmd("ip", &["link", "add", name, "type", "bridge"])
        .with_context(|| format!("failed to create bridge {name}"))?;

    // Assign the gateway IP.
    run_cmd(
        "ip",
        &["addr", "add", &format!("{gateway}/16"), "dev", name],
    )
    .with_context(|| format!("failed to assign IP to bridge {name}"))?;

    // Bring up the bridge.
    run_cmd("ip", &["link", "set", name, "up"])
        .with_context(|| format!("failed to bring up bridge {name}"))?;

    // Enable IP forwarding (required for NAT).
    let _ = fs::write("/proc/sys/net/ipv4/ip_forward", "1");

    // Add iptables MASQUERADE rule for NAT.
    // -t nat: operate on the NAT table
    // -A POSTROUTING: append to the postrouting chain
    // -s subnet: source address range
    // -j MASQUERADE: rewrite source IP to the host's outgoing IP
    let _ = run_cmd(
        "iptables",
        &[
            "-t",
            "nat",
            "-A",
            "POSTROUTING",
            "-s",
            DEFAULT_SUBNET,
            "!",
            "-o",
            name,
            "-j",
            "MASQUERADE",
        ],
    );

    Ok(())
}

/// Creates a veth pair and moves one end into the sandbox's network namespace.
#[allow(clippy::too_many_arguments)]
fn setup_veth_pair(
    sandbox_id: &str,
    veth_host: &str,
    veth_sandbox: &str,
    bridge: &str,
    ip: &str,
    gateway: &str,
    child_pid: Pid,
) -> Result<()> {
    let pid_str = child_pid.as_raw().to_string();

    // Use a temporary name for the sandbox-side veth to avoid conflicts
    // (e.g., "eth0" already exists on the host inside Docker).
    let veth_tmp = format!("vp-{}", &sandbox_id[..sandbox_id.len().min(6)]);

    // Create the veth pair on the host with a temporary peer name.
    run_cmd(
        "ip",
        &[
            "link", "add", veth_host, "type", "veth", "peer", "name", &veth_tmp,
        ],
    )
    .with_context(|| format!("failed to create veth pair for sandbox {sandbox_id}"))?;

    // Attach the host end to the bridge.
    run_cmd("ip", &["link", "set", veth_host, "master", bridge])
        .context("failed to attach veth to bridge")?;

    // Bring up the host end.
    run_cmd("ip", &["link", "set", veth_host, "up"]).context("failed to bring up host veth")?;

    // Move the sandbox end into the sandbox's network namespace.
    run_cmd("ip", &["link", "set", &veth_tmp, "netns", &pid_str])
        .context("failed to move veth into sandbox namespace")?;

    // Configure the sandbox end inside the namespace.
    let ns_path = format!("/proc/{}/ns/net", pid_str);

    // Rename the temporary interface to the final name (e.g., "eth0")
    // inside the sandbox namespace where there's no conflict.
    run_cmd(
        "nsenter",
        &[
            &format!("--net={ns_path}"),
            "ip",
            "link",
            "set",
            &veth_tmp,
            "name",
            veth_sandbox,
        ],
    )
    .context("failed to rename veth inside sandbox")?;

    // Bring up loopback.
    let _ = run_cmd(
        "nsenter",
        &[&format!("--net={ns_path}"), "ip", "link", "set", "lo", "up"],
    );

    // Assign IP address.
    run_cmd(
        "nsenter",
        &[
            &format!("--net={ns_path}"),
            "ip",
            "addr",
            "add",
            &format!("{ip}/16"),
            "dev",
            veth_sandbox,
        ],
    )
    .context("failed to assign IP inside sandbox")?;

    // Bring up the interface.
    run_cmd(
        "nsenter",
        &[
            &format!("--net={ns_path}"),
            "ip",
            "link",
            "set",
            veth_sandbox,
            "up",
        ],
    )
    .context("failed to bring up eth0 inside sandbox")?;

    // Set default route via the bridge gateway.
    run_cmd(
        "nsenter",
        &[
            &format!("--net={ns_path}"),
            "ip",
            "route",
            "add",
            "default",
            "via",
            gateway,
        ],
    )
    .context("failed to set default route inside sandbox")?;

    info!(
        sandbox = sandbox_id,
        ip, veth_host, bridge, "network configured"
    );

    Ok(())
}

/// Cleans up networking resources for a sandbox.
///
/// Deletes the host-side veth interface (which automatically removes the peer)
/// and releases the allocated IP address.
pub fn cleanup_network(sandbox_id: &str, info: &NetworkInfo) -> Result<()> {
    // Delete the host-side veth — the peer in the sandbox namespace is removed automatically.
    if let Some(ref veth) = info.veth_host {
        let _ = run_cmd("ip", &["link", "del", veth]);
        debug!(sandbox = sandbox_id, veth, "veth pair removed");
    }

    // Release the allocated IP.
    release_ip(sandbox_id)?;

    Ok(())
}

/// Allocates an IP address for a sandbox from the pool.
///
/// Uses a simple file-based allocator that tracks the next available IP.
/// IPs are allocated sequentially from 10.10.0.2 upward.
fn allocate_ip(sandbox_id: &str) -> Result<String> {
    let state_dir = Path::new(IP_STATE_FILE).parent().unwrap();
    fs::create_dir_all(state_dir).context("failed to create network state dir")?;

    // Read current state or start from 10.10.0.2.
    let state: IpAllocState = if Path::new(IP_STATE_FILE).exists() {
        let content = fs::read_to_string(IP_STATE_FILE).context("failed to read IP state")?;
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        IpAllocState::default()
    };

    // Find next available IP.
    let mut next = state.next_ip;
    // Skip .0 (network) and .1 (gateway).
    if next < 2 {
        next = 2;
    }

    let octet3 = (next / 256) as u8;
    let octet4 = (next % 256) as u8;
    let ip = format!("10.10.{octet3}.{octet4}");

    // Save updated state.
    let mut new_state = state;
    new_state.next_ip = next + 1;
    new_state
        .allocated
        .insert(sandbox_id.to_string(), ip.clone());

    let content =
        serde_json::to_string_pretty(&new_state).context("failed to serialize IP state")?;
    fs::write(IP_STATE_FILE, content).context("failed to write IP state")?;

    debug!(sandbox = sandbox_id, ip, "IP allocated");
    Ok(ip)
}

/// Releases an IP address back to the pool.
fn release_ip(sandbox_id: &str) -> Result<()> {
    if !Path::new(IP_STATE_FILE).exists() {
        return Ok(());
    }

    let content = fs::read_to_string(IP_STATE_FILE).context("failed to read IP state")?;
    let mut state: IpAllocState = serde_json::from_str(&content).unwrap_or_default();

    state.allocated.remove(sandbox_id);

    let content = serde_json::to_string_pretty(&state).context("failed to serialize IP state")?;
    fs::write(IP_STATE_FILE, content).context("failed to write IP state")?;

    Ok(())
}

/// Generates a resolv.conf file for the sandbox.
///
/// Returns the path to the generated file, which should be bind-mounted
/// into the sandbox at `/etc/resolv.conf`.
pub fn generate_resolv_conf(sandbox_id: &str) -> Result<PathBuf> {
    let path = PathBuf::from("/var/lib/hivebox/sandboxes")
        .join(sandbox_id)
        .join("resolv.conf");

    // Read the host's DNS config, filtering out loopback entries (e.g. Docker's
    // 127.0.0.11) which are unreachable from sandbox network namespaces.
    let content = fs::read_to_string("/etc/resolv.conf")
        .map(|c| {
            let filtered: String = c
                .lines()
                .filter(|line| {
                    if let Some(rest) = line.trim().strip_prefix("nameserver") {
                        !rest.trim().starts_with("127.")
                    } else {
                        true
                    }
                })
                .map(|l| format!("{l}\n"))
                .collect();
            if filtered.contains("nameserver") {
                filtered
            } else {
                "nameserver 8.8.8.8\nnameserver 1.1.1.1\n".to_string()
            }
        })
        .unwrap_or_else(|_| "nameserver 8.8.8.8\nnameserver 1.1.1.1\n".to_string());

    fs::write(&path, content).context("failed to write sandbox resolv.conf")?;

    Ok(path)
}

/// IP allocator state, persisted as JSON.
#[derive(Debug, Serialize, Deserialize)]
struct IpAllocState {
    /// Next IP offset to allocate (within 10.10.0.0/16 subnet).
    next_ip: u32,

    /// Currently allocated IPs: sandbox_id -> IP address.
    allocated: std::collections::HashMap<String, String>,
}

impl Default for IpAllocState {
    fn default() -> Self {
        Self {
            next_ip: 2,
            allocated: std::collections::HashMap::new(),
        }
    }
}

/// Runs an external command and returns Ok(()) if it exits with status 0.
fn run_cmd(program: &str, args: &[&str]) -> Result<()> {
    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("failed to execute: {} {}", program, args.join(" ")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "{} {} failed (exit {}): {}",
            program,
            args.join(" "),
            output.status.code().unwrap_or(-1),
            stderr.trim()
        );
    }

    Ok(())
}
