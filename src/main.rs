// Allow unused code warnings — many public APIs are defined for completeness
// and will be used as the project matures (images CLI, SSE streaming, etc.).
#![allow(dead_code, unused_imports)]

//! HiveBox — lightweight native sandboxing on Alpine Linux.
//!
//! This is the entry point for the `hivebox` binary, which serves as both
//! the CLI client and the daemon. It dispatches to the appropriate handler
//! based on the subcommand.
//!
//! # Modes of operation
//!
//! - **One-shot** (`hivebox run`): create sandbox, execute command, destroy.
//! - **Persistent** (`hivebox create/exec/destroy`): long-lived sandboxes.
//! - **Daemon** (`hivebox daemon`): REST API server for remote management.

mod api;
mod cli;
mod images;
mod mcp;
mod runtime;
mod sandbox;

use std::process;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use tracing::{error, info};

use cli::{Cli, Commands};
use sandbox::cgroup::{parse_memory_size, ResourceLimits};
use sandbox::manager::SandboxManager;
use sandbox::network::parse_network_mode;
use sandbox::SandboxConfig;

fn main() {
    let cli = Cli::parse();

    // Initialize structured logging.
    let filter = if cli.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(filter)),
        )
        .with_target(false)
        .init();

    // Dispatch to the appropriate handler.
    let result = match cli.command {
        Commands::Run(args) => handle_run(args),
        Commands::Create(args) => handle_create(args),
        Commands::Exec(args) => handle_exec(args),
        Commands::Destroy(args) => handle_destroy(args),
        Commands::List => handle_list(),
        Commands::Daemon(args) => handle_daemon(args),
        Commands::Mcp(args) => handle_mcp(args),
    };

    if let Err(e) = result {
        error!("{e:#}");
        process::exit(1);
    }
}

/// Handles `hivebox run` — one-shot sandbox execution.
fn handle_run(args: cli::RunArgs) -> Result<()> {
    let memory_bytes = parse_memory_size(&args.memory)
        .with_context(|| format!("invalid memory size: {}", args.memory))?;
    let network = parse_network_mode(&args.network)
        .with_context(|| format!("invalid network mode: {}", args.network))?;

    let command = args.command_string();
    let config = SandboxConfig {
        name: None,
        image: "base".to_string(),
        limits: ResourceLimits {
            memory_bytes,
            cpu_fraction: args.cpus,
            max_pids: args.pids,
        },
        network,
        command,
        skills: None,
        custom_mcps: None,
        llm_base_url: None,
        llm_api_key: None,
        llm_model: None,
        instructions: None,
    };

    let result = sandbox::create_and_run(&config).context("sandbox execution failed")?;

    if !result.stdout.is_empty() {
        print!("{}", result.stdout);
    }
    if !result.stderr.is_empty() {
        eprint!("{}", result.stderr);
    }

    if result.exit_code != 0 {
        process::exit(result.exit_code);
    }

    Ok(())
}

/// Handles `hivebox create` — create a persistent sandbox.
fn handle_create(args: cli::CreateArgs) -> Result<()> {
    let memory_bytes = parse_memory_size(&args.memory)?;
    let network = parse_network_mode(&args.network)?;

    let config = SandboxConfig {
        name: args.name.clone(),
        image: "base".to_string(),
        limits: ResourceLimits {
            memory_bytes,
            cpu_fraction: args.cpus,
            max_pids: args.pids,
        },
        network,
        command: String::new(),
        skills: None,
        custom_mcps: None,
        llm_base_url: None,
        llm_api_key: None,
        llm_model: None,
        instructions: None,
    };

    let rt = tokio::runtime::Runtime::new()?;
    let manager = Arc::new(SandboxManager::new());
    let sandbox_id = rt.block_on(manager.create(config, args.timeout))?;

    println!("{sandbox_id}");
    Ok(())
}

/// Handles `hivebox exec` — execute a command in an existing sandbox.
fn handle_exec(args: cli::ExecArgs) -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    let manager = Arc::new(SandboxManager::new());
    let command = args.command_string();

    let result = rt.block_on(manager.exec(&args.sandbox, &command))?;

    if !result.stdout.is_empty() {
        print!("{}", result.stdout);
    }
    if !result.stderr.is_empty() {
        eprint!("{}", result.stderr);
    }

    if result.exit_code != 0 {
        process::exit(result.exit_code);
    }

    Ok(())
}

/// Handles `hivebox destroy` — destroy a sandbox.
fn handle_destroy(args: cli::DestroyArgs) -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    let manager = Arc::new(SandboxManager::new());

    rt.block_on(manager.destroy(&args.sandbox))?;
    println!("Destroyed: {}", args.sandbox);

    Ok(())
}

/// Handles `hivebox list` — list all active sandboxes.
fn handle_list() -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    let manager = Arc::new(SandboxManager::new());
    let sandboxes = rt.block_on(manager.list());

    if sandboxes.is_empty() {
        println!("No active sandboxes.");
        return Ok(());
    }

    // Print a formatted table.
    println!(
        "{:<12} {:<10} {:<10} {:<10} {:<8} {:<10}",
        "ID", "STATUS", "UPTIME", "TTL", "CMDS", "NETWORK"
    );
    println!("{}", "-".repeat(60));

    for s in sandboxes {
        println!(
            "{:<12} {:<10} {:<10} {:<10} {:<8} {:<10}",
            s.id,
            format!("{:?}", s.state).to_lowercase(),
            format_duration(s.uptime_seconds),
            format_duration(s.ttl_seconds),
            s.commands_executed,
            s.network_mode,
        );
    }

    Ok(())
}

/// Handles `hivebox daemon` — start the API server.
fn handle_daemon(args: cli::DaemonArgs) -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;

    rt.block_on(async {
        if let Some(ref key) = args.api_key {
            info!("API key authentication enabled");
            api::start_server_with_auth(args.port, key.clone()).await
        } else {
            info!("WARNING: No API key set — authentication disabled");
            api::start_server(args.port).await
        }
    })
}

/// Handles `hivebox mcp` — run as MCP server for a sandbox.
fn handle_mcp(args: cli::McpArgs) -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(mcp::run(
        args.sandbox,
        args.api_url,
        args.api_key,
        args.skills_path,
    ))
}

/// Formats seconds into a human-readable duration string.
fn format_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m{}s", secs / 60, secs % 60)
    } else {
        format!("{}h{}m", secs / 3600, (secs % 3600) / 60)
    }
}
