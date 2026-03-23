//! Sandbox lifecycle manager.
//!
//! The `SandboxManager` is the central orchestrator for persistent sandboxes.
//! Unlike one-shot mode (`hivebox run`), persistent sandboxes stay alive between
//! commands: create → exec → exec → ... → destroy.
//!
//! # Key design: init process
//!
//! Each persistent sandbox has a minimal "init" process (PID 1 inside the namespace)
//! that just sleeps forever. This keeps the namespaces alive. Commands are executed
//! by entering the existing namespaces via `nsenter`/`setns` and spawning a new process.

use std::collections::HashMap;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{bail, Context, Result};
use serde::Serialize;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use super::cgroup::CgroupManager;
use super::filesystem::{cleanup_rootfs, prepare_rootfs};
use super::network::{self, NetworkInfo};
use super::{resolve_sandbox_id, SandboxConfig, SandboxState};

/// Maximum sandbox lifetime in seconds (24 hours).
const MAX_SANDBOX_LIFETIME: u64 = 24 * 60 * 60;

/// Default sandbox timeout in seconds if not specified.
const DEFAULT_TIMEOUT: u64 = 3600;

/// How often the reaper checks for expired sandboxes.
const REAPER_INTERVAL: u64 = 30;

/// How often metrics are sampled (seconds).
const METRICS_INTERVAL: u64 = 5;

/// Maximum number of history samples to keep (4320 = 6 hours at 5s interval).
const METRICS_HISTORY_MAX: usize = 4320;

/// Per-sandbox metric within a sample.
#[derive(Debug, Clone, Serialize)]
pub struct SandboxMetric {
    pub id: String,
    pub memory_bytes: u64,
    pub cpu_usec: u64,
    pub pids: u64,
}

/// A single metrics sample.
#[derive(Debug, Clone, Serialize)]
pub struct MetricsSample {
    pub timestamp: u64,
    pub total_memory_bytes: u64,
    pub total_cpu_usec: u64,
    pub total_pids: u64,
    pub sandbox_count: usize,
    pub sandboxes: Vec<SandboxMetric>,
    /// Host/container-level metrics from /proc.
    pub host_memory_total: u64,
    pub host_memory_used: u64,
    pub host_cpu_percent: f64,
}

/// Server-side analytics history.
#[derive(Debug, Clone, Serialize)]
pub struct AnalyticsHistory {
    pub samples: Vec<MetricsSample>,
    pub interval_secs: u64,
}

/// A persistent sandbox managed by the SandboxManager.
struct ManagedSandbox {
    id: String,
    state: SandboxState,
    config: SandboxConfig,
    init_pid: Option<i32>,
    created_at: Instant,
    created_at_str: String,
    expires_at: Instant,
    expires_at_str: String,
    network_info: Option<NetworkInfo>,
    rootfs_path: Option<PathBuf>,
    cwd: String,
    commands_executed: u64,
    /// Accumulated CPU microseconds from completed exec commands.
    /// Added to live namespace CPU to avoid losing short-lived command contributions.
    cumulative_cpu_usec: u64,
    /// Port of the opencode serve instance for this sandbox (if running).
    opencode_port: Option<u16>,
    /// PID of the opencode serve process (host PID, for cleanup).
    opencode_pid: Option<u32>,
    /// Path to the temporary opencode config directory.
    opencode_config_dir: Option<PathBuf>,
}

/// A file or directory entry returned by `list_files`.
#[derive(Debug, Clone, Serialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub entry_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

/// Public snapshot of sandbox state (for API responses).
#[derive(Debug, Clone)]
pub struct SandboxInfo {
    pub id: String,
    pub state: SandboxState,
    pub image: String,
    pub created_at: String,
    pub expires_at: String,
    pub uptime_seconds: u64,
    pub ttl_seconds: u64,
    pub network_mode: String,
    pub network_ip: Option<String>,
    pub memory_limit: String,
    pub cpu_limit: f64,
    pub pid_limit: u64,
    pub commands_executed: u64,
    pub memory_usage_bytes: u64,
    pub pid_current: u64,
    pub cpu_usage_usec: u64,
    /// Port of the opencode serve instance (if running).
    pub opencode_port: Option<u16>,
}

/// Configuration for the daemon, passed to the manager so it can spawn
/// opencode serve instances that connect back to the correct API endpoint.
#[derive(Clone)]
pub struct DaemonConfig {
    /// Port the hivebox daemon listens on.
    pub port: u16,
    /// API key (if auth is enabled).
    pub api_key: Option<String>,
    /// Whether to spawn opencode serve for each sandbox.
    pub opencode_enabled: bool,
    /// Source directory to copy skills from into each sandbox's opencode config.
    /// Default: `/root/.config/opencode/skills` (Anthropic skills downloaded at build time).
    /// Set `HIVEBOX_OPENCODE_SKILLS_PATH` to use a custom folder mounted into the container.
    pub skills_path: PathBuf,
    /// Global MCP servers (JSON object) added to every sandbox.
    /// Set via `HIVEBOX_OPENCODE_MCPS='{"name":{"type":"remote","url":"..."}}'`.
    pub global_mcps: Option<serde_json::Value>,
    /// Global LLM base URL. Set via `HIVEBOX_OPENCODE_BASE_URL`.
    pub llm_base_url: Option<String>,
    /// Global LLM API key. Set via `HIVEBOX_OPENCODE_API_KEY`.
    pub llm_api_key: Option<String>,
    /// Global LLM model. Set via `HIVEBOX_OPENCODE_MODEL`.
    pub llm_model: Option<String>,
    /// Pre-installed system packages (space-separated). Set via `HIVEBOX_PACKAGES`.
    pub installed_packages: Option<String>,
    /// Pre-installed pip packages (space-separated). Set via `HIVEBOX_PIP_PACKAGES`.
    pub installed_pip: Option<String>,
    /// Pre-installed npm packages (space-separated). Set via `HIVEBOX_NPM_PACKAGES`.
    pub installed_npm: Option<String>,
}

/// Base port for internal opencode serve instances.
const OPENCODE_BASE_PORT: u16 = 14000;

/// Thread-safe sandbox lifecycle manager.
pub struct SandboxManager {
    sandboxes: RwLock<HashMap<String, ManagedSandbox>>,
    metrics_history: RwLock<VecDeque<MetricsSample>>,
    /// Previous CPU jiffies snapshot for calculating CPU%.
    prev_cpu: RwLock<Option<(u64, u64)>>, // (total_jiffies, idle_jiffies)
    /// Next port to allocate for opencode serve instances.
    next_opencode_port: AtomicU16,
    /// Daemon configuration for opencode MCP bridge.
    daemon_config: DaemonConfig,
}

impl SandboxManager {
    pub fn new() -> Self {
        Self::with_config(DaemonConfig {
            port: 7070,
            api_key: None,
            opencode_enabled: true,
            skills_path: PathBuf::from("/opt/hivebox/skills"),
            global_mcps: None,
            llm_base_url: None,
            llm_api_key: None,
            llm_model: None,
            installed_packages: None,
            installed_pip: None,
            installed_npm: None,
        })
    }

    /// Returns the path to the skills directory on the host.
    pub fn skills_path(&self) -> &std::path::Path {
        &self.daemon_config.skills_path
    }

    /// Builds the MCP instructions string, including pre-installed package info.
    pub fn mcp_instructions(&self) -> String {
        let mut s = String::from(
            "HiveBox is a lightweight Linux sandbox running Debian Linux (glibc, apt package manager). \
             Commands run as root. The default working directory is /. ",
        );
        if let Some(ref pkgs) = self.daemon_config.installed_packages {
            s.push_str(&format!(
                "Pre-installed system packages (DO NOT reinstall): {}. ",
                pkgs
            ));
        }
        if let Some(ref pkgs) = self.daemon_config.installed_pip {
            s.push_str(&format!(
                "Pre-installed pip packages (already installed, DO NOT run pip install for these): {}. ",
                pkgs
            ));
        }
        if let Some(ref pkgs) = self.daemon_config.installed_npm {
            s.push_str(&format!(
                "Pre-installed npm global packages (already installed, DO NOT run npm install for these): {}. ",
                pkgs
            ));
        }
        if self.daemon_config.installed_packages.is_some()
            || self.daemon_config.installed_pip.is_some()
            || self.daemon_config.installed_npm.is_some()
        {
            s.push_str(
                "IMPORTANT: All packages listed above are already installed and ready to use. \
                 NEVER reinstall them. Just use them directly (e.g. require('pptxgenjs'), import docx, etc.). ",
            );
        }
        s.push_str(
            "When installing pip packages, always use: pip install --break-system-packages <pkg>. ",
        );
        s.push_str(
            "Use the 'exec' tool for shell commands and the file tools for reading/writing files.",
        );
        s
    }

    pub fn with_config(daemon_config: DaemonConfig) -> Self {
        Self {
            sandboxes: RwLock::new(HashMap::new()),
            metrics_history: RwLock::new(VecDeque::with_capacity(METRICS_HISTORY_MAX + 1)),
            prev_cpu: RwLock::new(None),
            next_opencode_port: AtomicU16::new(OPENCODE_BASE_PORT),
            daemon_config,
        }
    }

    /// Creates a new persistent sandbox.
    pub async fn create(&self, config: SandboxConfig, timeout_secs: u64) -> Result<String> {
        let sandbox_id = resolve_sandbox_id(config.name.as_deref());

        if self.sandboxes.read().await.contains_key(&sandbox_id) {
            bail!("sandbox '{}' already exists", sandbox_id);
        }

        let timeout = if timeout_secs == 0 {
            DEFAULT_TIMEOUT
        } else {
            timeout_secs.min(MAX_SANDBOX_LIFETIME)
        };

        info!(
            sandbox = sandbox_id,
            image = config.image,
            timeout_secs = timeout,
            "creating persistent sandbox"
        );

        let rootfs_path =
            prepare_rootfs(&sandbox_id, &config.image).context("failed to prepare rootfs")?;

        let init_pid = spawn_init_process(&sandbox_id, &rootfs_path, &config)?;

        let cgroup = CgroupManager::create(&sandbox_id)?;
        cgroup.apply_limits(&config.limits)?;
        cgroup.add_process(nix::unistd::Pid::from_raw(init_pid))?;

        let network_info = match network::setup_network(
            &sandbox_id,
            &config.network,
            nix::unistd::Pid::from_raw(init_pid),
        ) {
            Ok(info) => Some(info),
            Err(e) => {
                warn!(sandbox = sandbox_id, error = %e, "network setup failed");
                None
            }
        };

        if config.network != network::NetworkMode::None {
            let resolv_dst = rootfs_path.join("etc/resolv.conf");
            let _ = std::fs::write(&resolv_dst, "nameserver 8.8.8.8\nnameserver 1.1.1.1\n");
        }

        let now = Instant::now();
        let now_sys = SystemTime::now();
        let expires_at = now + Duration::from_secs(timeout);

        let sandbox = ManagedSandbox {
            id: sandbox_id.clone(),
            state: SandboxState::Running,
            config,
            init_pid: Some(init_pid),
            created_at: now,
            created_at_str: format_system_time(now_sys),
            expires_at,
            expires_at_str: format_system_time(now_sys + Duration::from_secs(timeout)),
            network_info,
            rootfs_path: Some(rootfs_path),
            cwd: "/".to_string(),
            commands_executed: 0,
            cumulative_cpu_usec: 0,
            opencode_port: None,
            opencode_pid: None,
            opencode_config_dir: None,
        };

        self.sandboxes
            .write()
            .await
            .insert(sandbox_id.clone(), sandbox);

        // Spawn an opencode serve instance for this sandbox (if enabled).
        if self.daemon_config.opencode_enabled {
            if let Err(e) = self.spawn_opencode(&sandbox_id).await {
                warn!(sandbox = sandbox_id, error = %e, "failed to spawn opencode serve (non-fatal)");
            }
        }

        info!(sandbox = sandbox_id, "sandbox created successfully");

        Ok(sandbox_id)
    }

    /// Spawns an `opencode serve` instance for a sandbox.
    ///
    /// Creates a temporary config directory with an `opencode.jsonc` that
    /// connects to this sandbox's MCP endpoint, then starts `opencode serve`
    /// on an auto-assigned port.
    ///
    /// Respects per-sandbox overrides for skills, MCPs, and LLM config,
    /// falling back to global defaults from `DaemonConfig` / env vars.
    async fn spawn_opencode(&self, sandbox_id: &str) -> Result<()> {
        let port = self.next_opencode_port.fetch_add(1, Ordering::Relaxed);
        let daemon_port = self.daemon_config.port;

        // Read sandbox-specific config before we build the opencode config.
        let (skills, custom_mcps, llm_base_url, llm_api_key, llm_model, custom_instructions) = {
            let sandboxes = self.sandboxes.read().await;
            let sb = sandboxes
                .get(sandbox_id)
                .context("sandbox not found while spawning opencode")?;
            (
                sb.config.skills.clone(),
                sb.config.custom_mcps.clone(),
                sb.config.llm_base_url.clone(),
                sb.config.llm_api_key.clone(),
                sb.config.llm_model.clone(),
                sb.config.instructions.clone(),
            )
        };

        // Build the MCP URL pointing back to this sandbox.
        let mcp_url = format!("http://127.0.0.1:{daemon_port}/api/v1/hiveboxes/{sandbox_id}/mcp");

        // Create temp config directory.
        let config_dir = PathBuf::from(format!("/tmp/hivebox-opencode/{sandbox_id}"));
        let opencode_dir = config_dir.join("opencode");
        std::fs::create_dir_all(&opencode_dir).with_context(|| {
            format!(
                "failed to create opencode config dir: {}",
                opencode_dir.display()
            )
        })?;

        // Create config_dir/.config/opencode → config_dir/opencode symlink.
        //
        // opencode may resolve skill paths via $HOME/.config/opencode/skills/ in some
        // code paths even when XDG_CONFIG_HOME is set, causing it to "see" skills
        // (via XDG) but fail to find their directories (via HOME).  By setting
        // HOME=config_dir and symlinking .config/opencode → opencode, both paths
        // resolve to the same directory and skill discovery is consistent.
        let home_config = config_dir.join(".config");
        std::fs::create_dir_all(&home_config)?;
        let home_opencode = home_config.join("opencode");
        if !home_opencode.exists() {
            std::os::unix::fs::symlink(&opencode_dir, &home_opencode).with_context(|| {
                format!(
                    "failed to create HOME/.config/opencode symlink: {}",
                    home_opencode.display()
                )
            })?;
        }

        // Build the opencode config JSON.
        let mut mcp_headers = serde_json::Map::new();
        if let Some(ref key) = self.daemon_config.api_key {
            mcp_headers.insert(
                "Authorization".to_string(),
                serde_json::Value::String(format!("Bearer {key}")),
            );
        }

        // Build instructions for the AI agent.
        let mut default_instructions = vec![
            format!("You are operating inside a HiveBox sandbox (ID: {sandbox_id}) running Debian Linux (glibc)."),
            "The sandbox has limited resources and no persistent storage — files are lost on destroy.".to_string(),
            "Use the MCP tools available to you (exec, read_file, write_file, etc.) to interact with the sandbox filesystem.".to_string(),
            "When installing pip packages, always use: pip install --break-system-packages <pkg>".to_string(),
        ];
        // Add pre-installed package info so the agent doesn't waste time reinstalling.
        if let Some(ref pkgs) = self.daemon_config.installed_packages {
            default_instructions.push(format!(
                "Pre-installed system packages (DO NOT reinstall): {}.",
                pkgs
            ));
        }
        if let Some(ref pkgs) = self.daemon_config.installed_pip {
            default_instructions.push(format!(
                "Pre-installed pip packages (already installed, DO NOT run pip install for these): {}.",
                pkgs
            ));
        }
        if let Some(ref pkgs) = self.daemon_config.installed_npm {
            default_instructions.push(format!(
                "Pre-installed npm global packages (already installed, DO NOT run npm install for these): {}.",
                pkgs
            ));
        }
        default_instructions.extend([
            "You have access to specialized skills. When a task involves a specific domain (PDF, PPTX, DOCX, XLSX, etc.), follow this workflow:".to_string(),
            "  1. Call list_skills to discover what skills are available.".to_string(),
            "  2. Call read_skill_file(skill, 'SKILL.md') to load the skill's instructions.".to_string(),
            "  3. If the skill references additional files (e.g. pptxgenjs.md, forms.md), call read_skill_file to read them.".to_string(),
            "  4. Follow the skill instructions exactly, using exec to run scripts.".to_string(),
            "Always load the relevant skill BEFORE attempting a specialized task.".to_string(),
            "To check if an npm package is installed: npm list -g <pkg> 2>/dev/null || npm install -g <pkg>".to_string(),
            "To check if a pip package is installed: python3 -c 'import <pkg>' 2>/dev/null || pip install --break-system-packages <pkg>".to_string(),
        ]);
        let mut instructions: Vec<String> = std::env::var("HIVEBOX_OPENCODE_INSTRUCTIONS")
            .map(|s| s.lines().map(|l| l.to_string()).collect())
            .unwrap_or(default_instructions);

        // Append per-sandbox custom instructions (from API request).
        if let Some(ref custom) = custom_instructions {
            instructions.extend(custom.iter().cloned());
        }

        // Build the MCP section: hivebox (always present) + global MCPs + per-sandbox MCPs.
        let mut mcp_section = serde_json::Map::new();
        mcp_section.insert(
            "hivebox".to_string(),
            serde_json::json!({
                "type": "remote",
                "url": mcp_url,
                "headers": mcp_headers,
                "enabled": true
            }),
        );

        // Merge global MCPs from HIVEBOX_MCPS env var.
        if let Some(ref global) = self.daemon_config.global_mcps {
            if let Some(map) = global.as_object() {
                for (key, value) in map {
                    if key != "hivebox" {
                        mcp_section.insert(key.clone(), value.clone());
                    }
                }
            }
        }

        // Merge per-sandbox custom MCPs (overrides globals, but never hivebox).
        if let Some(ref custom) = custom_mcps {
            if let Some(map) = custom.as_object() {
                for (key, value) in map {
                    if key != "hivebox" {
                        mcp_section.insert(key.clone(), value.clone());
                    }
                }
            }
        }

        // Build LLM provider config if base_url + model are provided.
        // Resolution: per-sandbox > global (DaemonConfig) > opencode defaults.
        let eff_base_url = llm_base_url
            .as_ref()
            .or(self.daemon_config.llm_base_url.as_ref());
        let eff_api_key = llm_api_key
            .as_ref()
            .or(self.daemon_config.llm_api_key.as_ref());
        let eff_model = llm_model.as_ref().or(self.daemon_config.llm_model.as_ref());

        // Write instructions to AGENTS.md so opencode picks them up as a rules file.
        let agents_md_path = opencode_dir.join("AGENTS.md");
        std::fs::write(&agents_md_path, instructions.join("\n"))
            .with_context(|| "failed to write AGENTS.md")?;

        let mut config_json = serde_json::json!({
            "$schema": "https://opencode.ai/config.json",
            "instructions": ["AGENTS.md"],
            "permission": {
                "*": "deny",
                "hivebox_*": "allow",
                "todowrite": "allow",
                "todoread": "allow",
                "task": "allow",
                "skill": "allow"
            },
            "mcp": mcp_section
        });

        // If LLM config is provided, generate a custom provider in the config file.
        // OpenCode expects: provider section + "model": "provider_id/model_id".
        if let (Some(base_url), Some(model_name)) = (eff_base_url, eff_model) {
            let mut provider_options = serde_json::json!({
                "baseURL": base_url
            });
            if let Some(key) = eff_api_key {
                provider_options["apiKey"] = serde_json::Value::String(key.clone());
            }

            config_json["provider"] = serde_json::json!({
                "hivebox-llm": {
                    "npm": "@ai-sdk/openai-compatible",
                    "name": "HiveBox LLM",
                    "options": provider_options,
                    "models": {
                        model_name: {
                            "name": model_name
                        }
                    }
                }
            });
            config_json["model"] = serde_json::Value::String(format!("hivebox-llm/{model_name}"));
        }

        let config_path = opencode_dir.join("opencode.jsonc");
        std::fs::write(&config_path, serde_json::to_string_pretty(&config_json)?).with_context(
            || format!("failed to write opencode config: {}", config_path.display()),
        )?;

        // Copy skills into the sandbox's opencode config directory.
        // Priority: per-sandbox skills list > global skills_path from config/env.
        let global_skills = &self.daemon_config.skills_path;
        let sandbox_skills = opencode_dir.join("skills");
        if global_skills.is_dir() {
            match &skills {
                None => {
                    // No skills specified: copy all from global.
                    copy_dir_recursive(global_skills, &sandbox_skills)
                        .with_context(|| "failed to copy skills")?;
                }
                Some(list) if list.is_empty() => {
                    // Explicitly empty: no skills at all.
                    debug!(sandbox = sandbox_id, "skills disabled for this sandbox");
                }
                Some(list) => {
                    // Specific skill names: copy only those that exist.
                    std::fs::create_dir_all(&sandbox_skills)?;
                    for name in list {
                        let src = global_skills.join(name);
                        if src.is_dir() {
                            copy_dir_recursive(&src, &sandbox_skills.join(name))?;
                        } else {
                            warn!(
                                sandbox = sandbox_id,
                                skill = name.as_str(),
                                "skill not found, skipping"
                            );
                        }
                    }
                }
            }
        }

        // Spawn opencode serve.
        let mut cmd = Command::new("opencode");
        cmd.args(["serve", "--port", &port.to_string()])
            .env("XDG_CONFIG_HOME", &config_dir)
            .env("HOME", &config_dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());

        let child = cmd
            .spawn()
            .with_context(|| "failed to spawn opencode serve — is opencode installed?")?;

        let pid = child.id();

        info!(
            sandbox = sandbox_id,
            opencode_port = port,
            opencode_pid = pid,
            "opencode serve started"
        );

        // Store in sandbox state.
        let mut sandboxes = self.sandboxes.write().await;
        if let Some(s) = sandboxes.get_mut(sandbox_id) {
            s.opencode_port = Some(port);
            s.opencode_pid = Some(pid);
            s.opencode_config_dir = Some(config_dir);
        }

        Ok(())
    }

    /// Returns the opencode serve port for a sandbox, if running.
    pub async fn get_opencode_port(&self, sandbox_id: &str) -> Option<u16> {
        let sandboxes = self.sandboxes.read().await;
        sandboxes.get(sandbox_id).and_then(|s| s.opencode_port)
    }

    /// Executes a command inside an existing sandbox.
    pub async fn exec(
        &self,
        sandbox_id: &str,
        command: &str,
    ) -> Result<crate::runtime::ExecResult> {
        let (init_pid, rootfs_path, cwd, limits) = {
            let sandboxes = self.sandboxes.read().await;
            let sandbox = sandboxes
                .get(sandbox_id)
                .ok_or_else(|| anyhow::anyhow!("sandbox '{}' not found", sandbox_id))?;

            if sandbox.state != SandboxState::Running {
                bail!(
                    "sandbox '{}' is not running (state: {:?})",
                    sandbox_id,
                    sandbox.state
                );
            }

            let pid = sandbox
                .init_pid
                .ok_or_else(|| anyhow::anyhow!("sandbox '{}' has no init process", sandbox_id))?;
            let rootfs = sandbox
                .rootfs_path
                .clone()
                .ok_or_else(|| anyhow::anyhow!("sandbox '{}' has no rootfs path", sandbox_id))?;
            (
                pid,
                rootfs,
                sandbox.cwd.clone(),
                sandbox.config.limits.clone(),
            )
        };

        debug!(sandbox = sandbox_id, cwd, command, "executing command");

        const CWD_MARKER: &str = "__HIVEBOX_CWD__";
        let rootfs_str = rootfs_path.display().to_string();

        // Enforce resource limits via ulimit (works even when cgroup controllers
        // aren't delegated, e.g. inside Docker). This is a defense-in-depth
        // measure — cgroup limits are still set if available.
        let nproc = limits.max_pids;
        // Node.js max-old-space-size = half of sandbox memory (in MB), minimum 128MB.
        let node_max_mb = std::cmp::max(128, limits.memory_bytes / 1024 / 1024 / 2);
        // ulimit -u = max user processes, -t = CPU time (seconds).
        // Note: ulimit -v (virtual memory) is NOT set because cgroups already limit
        // physical memory, and V8 needs large virtual address space reservations
        // (mmap) for its CodeRange that don't consume actual RAM.
        let cpu_secs = 3600; // 1 hour max CPU time per command
        let wrapped = format!(
            "export NODE_OPTIONS='--max-old-space-size={node_max_mb}'; export NODE_PATH=/usr/local/lib/node_modules:/usr/lib/node_modules; ulimit -u {nproc} 2>/dev/null; ulimit -t {cpu_secs} 2>/dev/null; cd {cwd} 2>/dev/null; {command}; echo {CWD_MARKER}$(pwd)"
        );
        let start = std::time::Instant::now();
        let child = Command::new("nsenter")
            .args([
                &format!("--target={init_pid}"),
                "--pid",
                "--uts",
                "--ipc",
                "--net",
                "--",
                "chroot",
                &rootfs_str,
                "/bin/sh",
                "-c",
                &wrapped,
            ])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .with_context(|| format!("failed to nsenter sandbox {sandbox_id}"))?;

        // Add the nsenter process (and its children) to the sandbox's cgroup
        // so resource usage is tracked correctly.
        let cg_for_exec = CgroupManager::open(sandbox_id).ok();
        if let Some(ref cg) = cg_for_exec {
            let _ = cg.add_process(nix::unistd::Pid::from_raw(child.id() as i32));
        }

        // Snapshot OOM kill count before command so we can detect new OOM kills.
        let oom_before = cg_for_exec.as_ref().map(|c| c.oom_kill_count()).unwrap_or(0);

        // Snapshot namespace CPU before command for delta tracking.
        let cpu_before = cpu_from_namespace(init_pid);

        let output = child
            .wait_with_output()
            .with_context(|| format!("failed to wait for nsenter in sandbox {sandbox_id}"))?;

        // After the command finishes, measure CPU delta from the command.
        // The nsenter'd process is gone, but we can compute the delta
        // from the namespace snapshot (which included it while running).
        let cpu_after = cpu_from_namespace(init_pid);
        // The command's CPU contribution: if cpu_after < cpu_before it means
        // the process exited and its CPU time is no longer visible, so we
        // estimate from wall time as a fallback.
        let duration_ms = start.elapsed().as_millis() as u64;
        let cmd_cpu_usec = if cpu_after >= cpu_before {
            cpu_after - cpu_before
        } else {
            // Process exited — its /proc entry is gone. Use duration as rough estimate.
            duration_ms * 1000
        };

        // Detect OOM kill: compare oom_kill count before/after command execution.
        let oom_after = cg_for_exec.as_ref().map(|c| c.oom_kill_count()).unwrap_or(0);
        let oom_killed = oom_after > oom_before;
        if oom_killed {
            warn!(
                sandbox = sandbox_id,
                oom_kills = oom_after - oom_before,
                "command was OOM-killed (memory limit exceeded)"
            );
        }

        let exit_code = output.status.code().unwrap_or(-1);
        let raw_stdout = String::from_utf8_lossy(&output.stdout).to_string();

        let (clean_stdout, new_cwd) = if let Some(pos) = raw_stdout.rfind(CWD_MARKER) {
            let before = &raw_stdout[..pos];
            let after = raw_stdout[pos + CWD_MARKER.len()..].trim();
            (before.to_string(), Some(after.to_string()))
        } else {
            (raw_stdout, None)
        };

        {
            let mut sandboxes = self.sandboxes.write().await;
            if let Some(s) = sandboxes.get_mut(sandbox_id) {
                s.commands_executed += 1;
                s.cumulative_cpu_usec += cmd_cpu_usec;
                if let Some(ref new) = new_cwd {
                    s.cwd = new.clone();
                }
            }
        }

        let mut stderr = String::from_utf8_lossy(&output.stderr).to_string();
        if oom_killed {
            if !stderr.is_empty() && !stderr.ends_with('\n') {
                stderr.push('\n');
            }
            let mem_mb = limits.memory_bytes / (1024 * 1024);
            stderr.push_str(&format!(
                "[hivebox] Process killed: out of memory (limit: {mem_mb} MiB). Use --memory to increase the limit."
            ));
        }

        Ok(crate::runtime::ExecResult {
            exit_code,
            stdout: clean_stdout,
            stderr,
            duration_ms,
            cwd: new_cwd,
            oom_killed,
        })
    }

    /// Destroys a sandbox, cleaning up all resources.
    pub async fn destroy(&self, sandbox_id: &str) -> Result<()> {
        let sandbox = self.sandboxes.write().await.remove(sandbox_id);

        let sandbox =
            sandbox.ok_or_else(|| anyhow::anyhow!("sandbox '{}' not found", sandbox_id))?;

        info!(sandbox = sandbox_id, "destroying sandbox");
        destroy_sandbox_resources(&sandbox);

        Ok(())
    }

    /// Gets information about a sandbox.
    pub async fn get(&self, sandbox_id: &str) -> Option<SandboxInfo> {
        let sandboxes = self.sandboxes.read().await;
        sandboxes.get(sandbox_id).map(sandbox_to_info)
    }

    /// Lists all sandboxes.
    pub async fn list(&self) -> Vec<SandboxInfo> {
        let sandboxes = self.sandboxes.read().await;
        sandboxes.values().map(sandbox_to_info).collect()
    }

    /// Writes a file into a sandbox's filesystem.
    pub async fn write_file(&self, sandbox_id: &str, path: &str, content: &[u8]) -> Result<()> {
        let sandboxes = self.sandboxes.read().await;
        let sandbox = sandboxes
            .get(sandbox_id)
            .ok_or_else(|| anyhow::anyhow!("sandbox '{}' not found", sandbox_id))?;

        let rootfs = sandbox
            .rootfs_path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("sandbox has no rootfs"))?;

        let full_path = rootfs.join(path.trim_start_matches('/'));
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&full_path, content)?;

        Ok(())
    }

    /// Reads a file from a sandbox's filesystem.
    pub async fn read_file(&self, sandbox_id: &str, path: &str) -> Result<Vec<u8>> {
        let sandboxes = self.sandboxes.read().await;
        let sandbox = sandboxes
            .get(sandbox_id)
            .ok_or_else(|| anyhow::anyhow!("sandbox '{}' not found", sandbox_id))?;

        let rootfs = sandbox
            .rootfs_path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("sandbox has no rootfs"))?;

        let full_path = rootfs.join(path.trim_start_matches('/'));
        let content = std::fs::read(&full_path)
            .with_context(|| format!("failed to read {}", full_path.display()))?;

        Ok(content)
    }

    /// List files and directories at a given path inside a sandbox.
    /// Returns entries with name, type (file/directory), and size.
    pub async fn list_files(&self, sandbox_id: &str, path: &str) -> Result<Vec<FileEntry>> {
        let sandboxes = self.sandboxes.read().await;
        let sandbox = sandboxes
            .get(sandbox_id)
            .ok_or_else(|| anyhow::anyhow!("sandbox '{}' not found", sandbox_id))?;

        let rootfs = sandbox
            .rootfs_path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("sandbox has no rootfs"))?;

        let full_path = rootfs.join(path.trim_start_matches('/'));
        if !full_path.is_dir() {
            bail!("path '{}' is not a directory", path);
        }

        let mut entries = Vec::new();
        Self::collect_entries(&full_path, path.trim_end_matches('/'), &mut entries)?;
        entries.sort_by(|a, b| a.path.cmp(&b.path));

        Ok(entries)
    }

    /// Recursively collect file entries from a directory.
    fn collect_entries(
        fs_path: &std::path::Path,
        logical_path: &str,
        entries: &mut Vec<FileEntry>,
    ) -> Result<()> {
        let read_dir = std::fs::read_dir(fs_path)
            .with_context(|| format!("failed to read directory {}", fs_path.display()))?;

        for entry in read_dir {
            let entry = entry?;
            let metadata = entry.metadata()?;
            let name = entry.file_name().to_string_lossy().to_string();
            let entry_logical = if logical_path.is_empty() {
                format!("/{name}")
            } else {
                format!("{logical_path}/{name}")
            };

            if metadata.is_dir() {
                entries.push(FileEntry {
                    name: name.clone(),
                    path: entry_logical.clone(),
                    entry_type: "directory".to_string(),
                    size: None,
                });
                Self::collect_entries(&entry.path(), &entry_logical, entries)?;
            } else if metadata.is_file() {
                entries.push(FileEntry {
                    name,
                    path: entry_logical,
                    entry_type: "file".to_string(),
                    size: Some(metadata.len()),
                });
            }
        }

        Ok(())
    }

    /// Background task that periodically destroys expired sandboxes.
    pub async fn run_reaper(self: &Arc<Self>) {
        loop {
            tokio::time::sleep(Duration::from_secs(REAPER_INTERVAL)).await;

            let expired: Vec<String> = {
                let sandboxes = self.sandboxes.read().await;
                let now = Instant::now();
                sandboxes
                    .values()
                    .filter(|s| now >= s.expires_at)
                    .map(|s| s.id.clone())
                    .collect()
            };

            for id in expired {
                warn!(sandbox = id, "sandbox expired, auto-destroying");
                if let Err(e) = self.destroy(&id).await {
                    error!(sandbox = id, error = %e, "failed to auto-destroy expired sandbox");
                }
            }
        }
    }

    /// Background task that samples metrics every METRICS_INTERVAL seconds.
    pub async fn run_metrics_collector(self: &Arc<Self>) {
        loop {
            tokio::time::sleep(Duration::from_secs(METRICS_INTERVAL)).await;

            let sandboxes = self.sandboxes.read().await;
            let mut total_mem: u64 = 0;
            let mut total_cpu: u64 = 0;
            let mut total_pids: u64 = 0;
            let count = sandboxes.len();
            let mut per_sandbox = Vec::new();

            for s in sandboxes.values() {
                // Memory: try cgroup, then cgroup.procs, then PID namespace scan.
                let cg = CgroupManager::open(&s.id).ok();
                let mut mem = cg.as_ref().and_then(|c| c.memory_usage().ok()).unwrap_or(0);
                if mem == 0 {
                    mem = memory_from_cgroup_procs(&s.id);
                }
                if mem == 0 {
                    if let Some(pid) = s.init_pid {
                        mem = memory_from_namespace(pid);
                    }
                }
                // CPU: try cgroup, then PID namespace scan; take the max.
                // Add cumulative CPU from completed commands that are no longer visible.
                let mut cpu = cg
                    .as_ref()
                    .and_then(|c| c.cpu_usage_usec().ok())
                    .unwrap_or(0);
                if let Some(pid) = s.init_pid {
                    let ns_cpu = cpu_from_namespace(pid);
                    if ns_cpu > cpu {
                        cpu = ns_cpu;
                    }
                }
                cpu += s.cumulative_cpu_usec;
                // PIDs: try cgroup, then PID namespace scan; take the max.
                let mut pids = cg.as_ref().and_then(|c| c.pid_count().ok()).unwrap_or(0);
                if let Some(pid) = s.init_pid {
                    let ns_pids = count_namespace_pids(pid);
                    if ns_pids > pids {
                        pids = ns_pids;
                    }
                }
                // Warn if memory usage exceeds 80% of limit.
                let limit = s.config.limits.memory_bytes;
                if limit > 0 && mem > 0 {
                    let pct = (mem as f64 / limit as f64) * 100.0;
                    if pct >= 80.0 {
                        warn!(
                            sandbox = s.id,
                            usage_mb = mem / (1024 * 1024),
                            limit_mb = limit / (1024 * 1024),
                            percent = format!("{:.0}", pct),
                            "memory usage above 80% — approaching OOM"
                        );
                    }
                }

                total_mem += mem;
                total_cpu += cpu;
                total_pids += pids;
                per_sandbox.push(SandboxMetric {
                    id: s.id.clone(),
                    memory_bytes: mem,
                    cpu_usec: cpu,
                    pids,
                });
            }
            drop(sandboxes);

            // Host-level metrics.
            let (host_mem_total, host_mem_used) = read_host_memory();
            let host_cpu_pct = {
                let cur = read_cpu_jiffies();
                let mut prev = self.prev_cpu.write().await;
                let pct = match (*prev, cur) {
                    (Some((prev_total, prev_idle)), Some((cur_total, cur_idle))) => {
                        let dt = cur_total.saturating_sub(prev_total) as f64;
                        let di = cur_idle.saturating_sub(prev_idle) as f64;
                        if dt > 0.0 {
                            ((dt - di) / dt) * 100.0
                        } else {
                            0.0
                        }
                    }
                    _ => 0.0,
                };
                *prev = cur;
                pct
            };

            let now = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            let sample = MetricsSample {
                timestamp: now,
                total_memory_bytes: total_mem,
                total_cpu_usec: total_cpu,
                total_pids,
                sandbox_count: count,
                sandboxes: per_sandbox,
                host_memory_total: host_mem_total,
                host_memory_used: host_mem_used,
                host_cpu_percent: host_cpu_pct,
            };

            let mut history = self.metrics_history.write().await;
            history.push_back(sample);
            if history.len() > METRICS_HISTORY_MAX {
                history.pop_front();
            }
        }
    }

    /// Returns the analytics history, optionally filtered by time range in seconds.
    pub async fn get_analytics(&self, range_secs: Option<u64>) -> AnalyticsHistory {
        let history = self.metrics_history.read().await;
        let samples: Vec<MetricsSample> = match range_secs {
            Some(range) => {
                let now = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let cutoff = now.saturating_sub(range);
                history
                    .iter()
                    .filter(|s| s.timestamp >= cutoff)
                    .cloned()
                    .collect()
            }
            None => history.iter().cloned().collect(),
        };
        AnalyticsHistory {
            samples,
            interval_secs: METRICS_INTERVAL,
        }
    }
}

/// Spawns a minimal init process inside new namespaces.
fn spawn_init_process(
    sandbox_id: &str,
    rootfs: &std::path::Path,
    _config: &SandboxConfig,
) -> Result<i32> {
    let child = Command::new("unshare")
        .args([
            "--pid",
            "--mount",
            "--uts",
            "--ipc",
            "--net",
            "--fork",
            &format!("--root={}", rootfs.display()),
            "/bin/sh",
            "-c",
            "mount -t proc proc /proc 2>/dev/null; mount -t devtmpfs devtmpfs /dev 2>/dev/null || { mkdir -p /dev; mknod -m 666 /dev/null c 1 3 2>/dev/null; mknod -m 666 /dev/zero c 1 5 2>/dev/null; mknod -m 666 /dev/urandom c 1 9 2>/dev/null; mknod -m 666 /dev/random c 1 8 2>/dev/null; mknod -m 666 /dev/tty c 5 0 2>/dev/null; }; exec sleep infinity",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .with_context(|| format!("failed to spawn init for sandbox {sandbox_id}"))?;

    let unshare_pid = child.id() as i32;

    let init_pid = {
        let mut child_pid = None;
        for _ in 0..50 {
            let children_path = format!("/proc/{unshare_pid}/task/{unshare_pid}/children");
            if let Ok(contents) = std::fs::read_to_string(&children_path) {
                let trimmed = contents.trim();
                if !trimmed.is_empty() {
                    if let Some(first) = trimmed.split_whitespace().next() {
                        if let Ok(pid) = first.parse::<i32>() {
                            child_pid = Some(pid);
                            break;
                        }
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        child_pid.unwrap_or(unshare_pid)
    };

    info!(
        sandbox = sandbox_id,
        init_pid,
        unshare_pid,
        rootfs = %rootfs.display(),
        "init process spawned"
    );

    Ok(init_pid)
}

/// Cleans up all resources for a sandbox.
fn destroy_sandbox_resources(sandbox: &ManagedSandbox) {
    // Kill opencode serve process if running.
    if let Some(pid) = sandbox.opencode_pid {
        info!(
            sandbox = sandbox.id,
            opencode_pid = pid,
            "killing opencode serve"
        );
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(pid as i32),
            nix::sys::signal::Signal::SIGTERM,
        );
    }

    // Clean up opencode config directory.
    if let Some(ref dir) = sandbox.opencode_config_dir {
        let _ = std::fs::remove_dir_all(dir);
    }

    if let Some(pid) = sandbox.init_pid {
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(pid),
            nix::sys::signal::Signal::SIGKILL,
        );
        std::thread::sleep(Duration::from_millis(100));
    }

    if let Ok(cgroup) = CgroupManager::create(&sandbox.id) {
        let _ = cgroup.kill_all();
        std::thread::sleep(Duration::from_millis(50));
        let _ = cgroup.cleanup();
    }

    if let Some(ref net_info) = sandbox.network_info {
        let _ = network::cleanup_network(&sandbox.id, net_info);
    }

    let _ = cleanup_rootfs(&sandbox.id);

    info!(sandbox = sandbox.id, "sandbox resources cleaned up");
}

/// Converts a ManagedSandbox to a public SandboxInfo.
fn sandbox_to_info(s: &ManagedSandbox) -> SandboxInfo {
    let now = Instant::now();
    let ttl = if s.expires_at > now {
        (s.expires_at - now).as_secs()
    } else {
        0
    };

    let (mem_usage, pid_cur, cpu_usec) = {
        let cg = CgroupManager::open(&s.id).ok();
        let mut mem = cg.as_ref().and_then(|c| c.memory_usage().ok()).unwrap_or(0);
        if mem == 0 {
            mem = memory_from_cgroup_procs(&s.id);
        }
        if mem == 0 {
            if let Some(pid) = s.init_pid {
                mem = memory_from_namespace(pid);
            }
        }
        let mut pids = cg.as_ref().and_then(|c| c.pid_count().ok()).unwrap_or(0);
        if let Some(pid) = s.init_pid {
            let ns_pids = count_namespace_pids(pid);
            if ns_pids > pids {
                pids = ns_pids;
            }
        }
        let mut cpu = cg
            .as_ref()
            .and_then(|c| c.cpu_usage_usec().ok())
            .unwrap_or(0);
        if let Some(pid) = s.init_pid {
            let ns_cpu = cpu_from_namespace(pid);
            if ns_cpu > cpu {
                cpu = ns_cpu;
            }
        }
        cpu += s.cumulative_cpu_usec;
        (mem, pids, cpu)
    };

    SandboxInfo {
        id: s.id.clone(),
        state: s.state,
        image: s.config.image.clone(),
        created_at: s.created_at_str.clone(),
        expires_at: s.expires_at_str.clone(),
        uptime_seconds: s.created_at.elapsed().as_secs(),
        ttl_seconds: ttl,
        network_mode: s.config.network.to_string(),
        network_ip: s.network_info.as_ref().and_then(|n| n.ip.clone()),
        memory_limit: format_bytes(s.config.limits.memory_bytes),
        cpu_limit: s.config.limits.cpu_fraction,
        pid_limit: s.config.limits.max_pids,
        commands_executed: s.commands_executed,
        memory_usage_bytes: mem_usage,
        pid_current: pid_cur,
        cpu_usage_usec: cpu_usec,
        opencode_port: s.opencode_port,
    }
}

/// Recursively copy a directory tree.
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dest = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), &dest)?;
        }
    }
    Ok(())
}

fn format_bytes(bytes: u64) -> String {
    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.1}G", bytes as f64 / GB as f64)
    } else {
        format!("{}M", bytes / MB)
    }
}

/// Reads RSS memory of all processes in a sandbox's cgroup via /proc/{pid}/statm.
/// Fallback for when the cgroup memory controller isn't delegated (e.g. inside Docker).
fn memory_from_cgroup_procs(sandbox_id: &str) -> u64 {
    let procs_path = format!("/sys/fs/cgroup/hivebox/{sandbox_id}/cgroup.procs");
    let content: String = match std::fs::read_to_string(&procs_path) {
        Ok(c) => c,
        Err(_) => return 0,
    };

    let page_size: u64 = 4096;
    let mut total_rss: u64 = 0;

    for line in content.lines() {
        let pid = match line.trim().parse::<u32>() {
            Ok(p) => p,
            Err(_) => continue,
        };
        let statm_path = format!("/proc/{pid}/statm");
        if let Ok(statm) = std::fs::read_to_string(&statm_path) {
            // statm format: size resident shared text lib data dt (all in pages)
            let fields: Vec<&str> = statm.split_whitespace().collect();
            if fields.len() >= 2 {
                if let Ok(rss_pages) = fields[1].parse::<u64>() {
                    total_rss += rss_pages * page_size;
                }
            }
        }
    }
    total_rss
}

/// Finds all host PIDs that share the same PID namespace as init_pid.
/// This catches nsenter'd processes which are NOT children of init_pid in the
/// host process tree — they're children of the hivebox server but they entered
/// the sandbox's PID namespace via setns().
fn pids_in_namespace(init_pid: i32) -> Vec<i32> {
    let ns_path = format!("/proc/{init_pid}/ns/pid");
    let target_ns = match std::fs::read_link(&ns_path) {
        Ok(ns) => ns,
        Err(_) => return vec![init_pid],
    };

    let mut result = Vec::new();
    let proc_dir = match std::fs::read_dir("/proc") {
        Ok(d) => d,
        Err(_) => return vec![init_pid],
    };

    for entry in proc_dir.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if let Ok(pid) = name_str.parse::<i32>() {
            let pid_ns = format!("/proc/{pid}/ns/pid");
            if let Ok(ns) = std::fs::read_link(&pid_ns) {
                if ns == target_ns {
                    result.push(pid);
                }
            }
        }
    }

    if result.is_empty() {
        result.push(init_pid);
    }
    result
}

/// Reads RSS memory of all processes in the same PID namespace as init_pid.
fn memory_from_namespace(init_pid: i32) -> u64 {
    let page_size: u64 = 4096;
    let mut total_rss: u64 = 0;

    for pid in pids_in_namespace(init_pid) {
        let statm_path = format!("/proc/{pid}/statm");
        if let Ok(statm) = std::fs::read_to_string(&statm_path) {
            let fields: Vec<&str> = statm.split_whitespace().collect();
            if fields.len() >= 2 {
                if let Ok(rss_pages) = fields[1].parse::<u64>() {
                    total_rss += rss_pages * page_size;
                }
            }
        }
    }
    total_rss
}

/// Reads cumulative CPU time (utime + stime) from all processes in the same
/// PID namespace as init_pid. Returns total in microseconds.
fn cpu_from_namespace(init_pid: i32) -> u64 {
    let ticks_per_sec: u64 = 100;
    let mut total_usec: u64 = 0;

    for pid in pids_in_namespace(init_pid) {
        let stat_path = format!("/proc/{pid}/stat");
        if let Ok(stat) = std::fs::read_to_string(&stat_path) {
            if let Some(after_comm) = stat.rfind(')') {
                let fields: Vec<&str> = stat[after_comm + 2..].split_whitespace().collect();
                if fields.len() > 12 {
                    let utime = fields[11].parse::<u64>().unwrap_or(0);
                    let stime = fields[12].parse::<u64>().unwrap_or(0);
                    total_usec += (utime + stime) * 1_000_000 / ticks_per_sec;
                }
            }
        }
    }
    total_usec
}

/// Counts processes in the same PID namespace as init_pid.
fn count_namespace_pids(init_pid: i32) -> u64 {
    pids_in_namespace(init_pid).len() as u64
}

/// Reads host/container memory from /proc/meminfo.
/// Returns (total_bytes, used_bytes).
fn read_host_memory() -> (u64, u64) {
    let content: String = match std::fs::read_to_string("/proc/meminfo") {
        Ok(c) => c,
        Err(_) => return (0, 0),
    };

    let mut total: u64 = 0;
    let mut available: u64 = 0;

    for line in content.lines() {
        if let Some(val) = line.strip_prefix("MemTotal:") {
            total = parse_meminfo_kb(val) * 1024;
        } else if let Some(val) = line.strip_prefix("MemAvailable:") {
            available = parse_meminfo_kb(val) * 1024;
        }
    }
    (total, total.saturating_sub(available))
}

fn parse_meminfo_kb(s: &str) -> u64 {
    s.trim()
        .trim_end_matches("kB")
        .trim()
        .parse::<u64>()
        .unwrap_or(0)
}

/// Reads CPU jiffies from /proc/stat.
/// Returns Some((total_jiffies, idle_jiffies)).
fn read_cpu_jiffies() -> Option<(u64, u64)> {
    let content: String = std::fs::read_to_string("/proc/stat").ok()?;
    let first_line = content.lines().next()?;
    if !first_line.starts_with("cpu ") {
        return None;
    }
    let fields: Vec<u64> = first_line[4..]
        .split_whitespace()
        .filter_map(|f| f.parse::<u64>().ok())
        .collect();
    if fields.len() < 4 {
        return None;
    }
    // fields: user, nice, system, idle, iowait, irq, softirq, steal, ...
    let total: u64 = fields.iter().sum();
    let idle = fields[3] + fields.get(4).copied().unwrap_or(0); // idle + iowait
    Some((total, idle))
}

fn format_system_time(t: SystemTime) -> String {
    let duration = t.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
    let secs = duration.as_secs();

    let days = secs / 86400;
    let remaining = secs % 86400;
    let hours = remaining / 3600;
    let mins = (remaining % 3600) / 60;
    let s = remaining % 60;

    let mut year = 1970u64;
    let mut day_count = days;
    loop {
        let days_in_year =
            if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) {
                366
            } else {
                365
            };
        if day_count < days_in_year {
            break;
        }
        day_count -= days_in_year;
        year += 1;
    }

    let months: &[u64] =
        if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) {
            &[31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
        } else {
            &[31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
        };

    let mut month = 0u64;
    for &m_days in months {
        if day_count < m_days {
            break;
        }
        day_count -= m_days;
        month += 1;
    }

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year,
        month + 1,
        day_count + 1,
        hours,
        mins,
        s
    )
}
