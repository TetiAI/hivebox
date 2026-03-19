//! Command-line interface for HiveBox.
//!
//! The CLI is built with `clap` and provides user-facing commands for
//! managing sandboxes. It supports both one-shot execution and persistent
//! sandbox workflows (create → exec → destroy).
//!
//! # Usage examples
//!
//! ```bash
//! # One-shot: create sandbox, run command, destroy
//! hivebox run -- echo "hello from the sandbox"
//!
//! # Persistent sandbox workflow
//! hivebox create --name myagent
//! hivebox exec myagent -- pip install requests
//! hivebox exec myagent -- python3 script.py
//! hivebox list
//! hivebox destroy myagent
//!
//! # Start the daemon
//! hivebox daemon --port 7070 --api-key mysecretkey
//! ```

use clap::{Parser, Subcommand};

/// HiveBox — lightweight native sandboxing on Alpine Linux.
///
/// Creates isolated execution environments using Linux kernel primitives
/// (namespaces, cgroups, seccomp) with near-zero overhead.
#[derive(Parser, Debug)]
#[command(name = "hivebox")]
#[command(version, about, long_about = None)]
pub struct Cli {
    /// Enable verbose logging (set RUST_LOG=debug for more detail).
    #[arg(short, long, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Commands,
}

/// Available commands.
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Run a command in a new sandbox (one-shot: create, execute, destroy).
    Run(RunArgs),

    /// Create a persistent sandbox (stays alive until destroyed or timeout).
    Create(CreateArgs),

    /// Execute a command in an existing sandbox.
    Exec(ExecArgs),

    /// Destroy a sandbox and clean up all resources.
    Destroy(DestroyArgs),

    /// List all active sandboxes with status and resource usage.
    #[command(alias = "ls")]
    List,

    /// Start the HiveBox daemon (API server).
    Daemon(DaemonArgs),

    /// Run as an MCP server (stdio) for a specific sandbox.
    ///
    /// Exposes sandbox tools (exec, read/write files, search, etc.) via
    /// the Model Context Protocol over stdin/stdout. Designed to be spawned
    /// by OpenCode or any MCP-compatible client.
    ///
    /// Example: hivebox mcp --sandbox abc123 --api-url http://localhost:7070
    Mcp(McpArgs),
}

/// Arguments for the `run` command (one-shot sandbox execution).
#[derive(Parser, Debug)]
pub struct RunArgs {
    /// Memory limit (e.g., "256m", "1g", "512k").
    #[arg(long, default_value = "256m")]
    pub memory: String,

    /// CPU limit as a fraction of one core.
    #[arg(long, default_value = "1.0")]
    pub cpus: f64,

    /// Maximum number of processes (prevents fork bombs).
    #[arg(long, default_value = "64")]
    pub pids: u64,

    /// Network mode: none, isolated, or shared:groupname.
    #[arg(long, default_value = "none")]
    pub network: String,

    /// Command to execute inside the sandbox.
    #[arg(trailing_var_arg = true, required = true)]
    pub command: Vec<String>,
}

impl RunArgs {
    /// Joins the command arguments into a single shell command string.
    pub fn command_string(&self) -> String {
        self.command.join(" ")
    }
}

/// Arguments for the `create` command (persistent sandbox).
#[derive(Parser, Debug)]
pub struct CreateArgs {
    /// Name for the sandbox. If not provided, a random ID is generated.
    /// Use this name to reference the sandbox in exec/destroy commands.
    #[arg(long)]
    pub name: Option<String>,

    /// Memory limit.
    #[arg(long, default_value = "256m")]
    pub memory: String,

    /// CPU limit as a fraction of one core.
    #[arg(long, default_value = "1.0")]
    pub cpus: f64,

    /// Maximum number of processes.
    #[arg(long, default_value = "64")]
    pub pids: u64,

    /// Network mode: none, isolated, or shared:groupname.
    #[arg(long, default_value = "none")]
    pub network: String,

    /// Maximum sandbox lifetime in seconds. The sandbox is automatically
    /// destroyed after this time. Cannot exceed 24 hours (86400s).
    #[arg(long, default_value = "3600")]
    pub timeout: u64,
}

/// Arguments for the `exec` command.
#[derive(Parser, Debug)]
pub struct ExecArgs {
    /// Sandbox name or ID to execute the command in.
    pub sandbox: String,

    /// Command to execute inside the sandbox.
    #[arg(trailing_var_arg = true, required = true)]
    pub command: Vec<String>,
}

impl ExecArgs {
    /// Joins the command arguments into a single shell command string.
    pub fn command_string(&self) -> String {
        self.command.join(" ")
    }
}

/// Arguments for the `destroy` command.
#[derive(Parser, Debug)]
pub struct DestroyArgs {
    /// Sandbox name or ID to destroy.
    pub sandbox: String,
}

/// Arguments for the `mcp` command (MCP stdio server for a sandbox).
#[derive(Parser, Debug)]
pub struct McpArgs {
    /// Sandbox name or ID to expose via MCP.
    #[arg(long)]
    pub sandbox: String,

    /// HiveBox daemon API URL.
    #[arg(long, default_value = "http://localhost:7070", env = "HIVEBOX_API_URL")]
    pub api_url: String,

    /// HiveBox daemon API key (if auth is enabled).
    #[arg(long, env = "HIVEBOX_API_KEY")]
    pub api_key: Option<String>,

    /// Path to the skills directory on the host.
    #[arg(
        long,
        default_value = "/opt/hivebox/skills",
        env = "HIVEBOX_OPENCODE_SKILLS_PATH"
    )]
    pub skills_path: std::path::PathBuf,
}

/// Arguments for the `daemon` command.
#[derive(Parser, Debug)]
pub struct DaemonArgs {
    /// TCP port to listen on for the REST API.
    #[arg(long, default_value = "7070")]
    pub port: u16,

    /// API key required for authentication. Requests must include
    /// `Authorization: Bearer <key>` header. If not set, reads from
    /// HIVEBOX_API_KEY environment variable. If neither is set,
    /// authentication is disabled (NOT recommended for production).
    #[arg(long, env = "HIVEBOX_API_KEY")]
    pub api_key: Option<String>,
}
