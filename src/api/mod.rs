//! REST API server for HiveBox.
//!
//! Provides HTTP endpoints for managing sandboxes remotely. The API follows
//! REST conventions with JSON request/response bodies.
//!
//! # Endpoints
//!
//! | Method | Path                          | Description              |
//! |--------|-------------------------------|--------------------------|
//! | POST   | /api/v1/hiveboxes             | Create a new sandbox     |
//! | GET    | /api/v1/hiveboxes             | List all sandboxes       |
//! | GET    | /api/v1/hiveboxes/:id         | Get sandbox details      |
//! | POST   | /api/v1/hiveboxes/:id/exec    | Execute command          |
//! | POST   | /api/v1/hiveboxes/:id/mcp     | MCP over HTTP            |
//! | PUT    | /api/v1/hiveboxes/:id/files      | Upload file              |
//! | GET    | /api/v1/hiveboxes/:id/files      | Download file            |
//! | GET    | /api/v1/hiveboxes/:id/files/list | List files recursively   |
//! | DELETE | /api/v1/hiveboxes/:id            | Destroy sandbox          |
//!
//! # Server configuration
//!
//! The API server listens on TCP port 7070 (configurable).

pub mod dashboard;
pub mod handlers;
pub mod mcp;
pub mod opencode;
pub mod types;

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{any, delete, get, post, put};
use axum::Router;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::info;

use crate::sandbox::manager::{DaemonConfig, SandboxManager};
use handlers::{AppState, SharedState};

/// Reads global opencode configuration from environment variables.
fn daemon_config_from_env(port: u16, api_key: Option<String>) -> DaemonConfig {
    let opencode_enabled = std::env::var("HIVEBOX_OPENCODE")
        .map(|v| v != "false" && v != "0")
        .unwrap_or(true);

    let skills_path = std::env::var("HIVEBOX_OPENCODE_SKILLS_PATH")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/opt/hivebox/skills"));

    let global_mcps = std::env::var("HIVEBOX_OPENCODE_MCPS")
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok());

    let llm_base_url = std::env::var("HIVEBOX_OPENCODE_BASE_URL").ok();
    let llm_api_key = std::env::var("HIVEBOX_OPENCODE_API_KEY").ok();
    let llm_model = std::env::var("HIVEBOX_OPENCODE_MODEL").ok();

    let installed_packages = std::env::var("HIVEBOX_PACKAGES").ok();
    let installed_pip = std::env::var("HIVEBOX_PIP_PACKAGES").ok();
    let installed_npm = std::env::var("HIVEBOX_NPM_PACKAGES").ok();

    DaemonConfig {
        port,
        api_key,
        opencode_enabled,
        skills_path,
        global_mcps,
        llm_base_url,
        llm_api_key,
        llm_model,
        installed_packages,
        installed_pip,
        installed_npm,
    }
}

/// Default TCP port for the REST API.
pub const DEFAULT_PORT: u16 = 7070;

/// Builds the axum router with all API routes.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/hiveboxes", post(handlers::create_sandbox))
        .route("/api/v1/hiveboxes", get(handlers::list_sandboxes))
        .route("/api/v1/hiveboxes/{id}", get(handlers::get_sandbox))
        .route("/api/v1/hiveboxes/{id}", delete(handlers::destroy_sandbox))
        .route(
            "/api/v1/hiveboxes/{id}/exec",
            post(handlers::exec_in_sandbox),
        )
        .route("/api/v1/hiveboxes/{id}/mcp", post(mcp::mcp_handler))
        .route("/api/v1/hiveboxes/{id}/files", put(handlers::upload_file))
        .route("/api/v1/hiveboxes/{id}/files", get(handlers::download_file))
        .route(
            "/api/v1/hiveboxes/{id}/files/list",
            get(handlers::list_files),
        )
        .route(
            "/api/v1/hiveboxes/{id}/opencode/{*rest}",
            any(opencode::opencode_proxy),
        )
        .route("/api/v1/analytics", get(handlers::get_analytics))
        .route("/healthz", get(|| async { "ok" }))
        .route("/dashboard", get(dashboard::dashboard_page))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// Starts the API server without authentication.
pub async fn start_server(port: u16) -> anyhow::Result<()> {
    let daemon_config = daemon_config_from_env(port, None);
    let manager = Arc::new(SandboxManager::with_config(daemon_config));

    let reaper_manager = manager.clone();
    tokio::spawn(async move {
        reaper_manager.run_reaper().await;
    });

    let metrics_manager = manager.clone();
    tokio::spawn(async move {
        metrics_manager.run_metrics_collector().await;
    });

    let state = SharedState { manager };
    let router = build_router(state);
    let addr = format!("0.0.0.0:{port}");

    info!(addr, "starting HiveBox API server (no auth)");

    let listener = TcpListener::bind(&addr).await?;
    axum::serve(listener, router).await?;

    Ok(())
}

/// Starts the API server with Bearer token authentication.
pub async fn start_server_with_auth(port: u16, api_key: String) -> anyhow::Result<()> {
    let daemon_config = daemon_config_from_env(port, Some(api_key.clone()));
    let manager = Arc::new(SandboxManager::with_config(daemon_config));

    let reaper_manager = manager.clone();
    tokio::spawn(async move {
        reaper_manager.run_reaper().await;
    });

    let metrics_manager = manager.clone();
    tokio::spawn(async move {
        metrics_manager.run_metrics_collector().await;
    });

    let state = SharedState { manager };
    let router = build_router_with_auth(state, api_key);
    let addr = format!("0.0.0.0:{port}");

    info!(addr, "starting HiveBox API server (auth enabled)");

    let listener = TcpListener::bind(&addr).await?;
    axum::serve(listener, router).await?;

    Ok(())
}

/// Builds the router with API key authentication middleware.
fn build_router_with_auth(state: AppState, api_key: String) -> Router {
    let api_routes = Router::new()
        .route("/api/v1/hiveboxes", post(handlers::create_sandbox))
        .route("/api/v1/hiveboxes", get(handlers::list_sandboxes))
        .route("/api/v1/hiveboxes/{id}", get(handlers::get_sandbox))
        .route("/api/v1/hiveboxes/{id}", delete(handlers::destroy_sandbox))
        .route(
            "/api/v1/hiveboxes/{id}/exec",
            post(handlers::exec_in_sandbox),
        )
        .route("/api/v1/hiveboxes/{id}/mcp", post(mcp::mcp_handler))
        .route("/api/v1/hiveboxes/{id}/files", put(handlers::upload_file))
        .route("/api/v1/hiveboxes/{id}/files", get(handlers::download_file))
        .route(
            "/api/v1/hiveboxes/{id}/files/list",
            get(handlers::list_files),
        )
        .route(
            "/api/v1/hiveboxes/{id}/opencode/{*rest}",
            any(opencode::opencode_proxy),
        )
        .route("/api/v1/analytics", get(handlers::get_analytics))
        .layer(middleware::from_fn(move |req, next| {
            let key = api_key.clone();
            auth_middleware(req, next, key)
        }))
        .with_state(state);

    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/dashboard", get(dashboard::dashboard_page))
        .merge(api_routes)
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
}

/// Middleware that validates the `Authorization: Bearer <key>` header.
async fn auth_middleware(
    req: Request,
    next: Next,
    expected_key: String,
) -> Result<Response, StatusCode> {
    let auth_header = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok());

    match auth_header {
        Some(header) if header.starts_with("Bearer ") => {
            let token = &header[7..];
            if token == expected_key {
                Ok(next.run(req).await)
            } else {
                Err(StatusCode::UNAUTHORIZED)
            }
        }
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}
