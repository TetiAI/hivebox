//! REST API endpoint handlers.
//!
//! Each handler corresponds to one API endpoint. They receive the shared
//! `AppState` (containing the `SandboxManager`) via axum's state extraction,
//! and return JSON responses.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use tracing::{error, info};

use super::types::*;
use crate::sandbox::cgroup::{parse_memory_size, ResourceLimits};
use crate::sandbox::manager::{AnalyticsHistory, SandboxInfo, SandboxManager};
use crate::sandbox::SandboxConfig;

/// Shared application state passed to all handlers.
#[derive(Clone)]
pub struct SharedState {
    pub manager: Arc<SandboxManager>,
}

pub type AppState = SharedState;

/// `POST /api/v1/hiveboxes` — Create a new sandbox.
pub async fn create_sandbox(
    State(state): State<AppState>,
    Json(req): Json<CreateSandboxRequest>,
) -> Result<(StatusCode, Json<CreateSandboxResponse>), (StatusCode, Json<ErrorResponse>)> {
    let manager = &state.manager;
    let memory_bytes = parse_memory_size(&req.memory).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("invalid memory size: {e}"),
            }),
        )
    })?;

    let config = SandboxConfig {
        name: req.name.clone(),
        image: "base".to_string(),
        limits: ResourceLimits {
            memory_bytes,
            cpu_fraction: req.cpus,
            max_pids: req.pids,
        },
        network: req.network.clone(),
        command: String::new(),
        skills: req.skills,
        custom_mcps: req.custom_mcps,
        llm_base_url: req.llm_base_url,
        llm_api_key: req.llm_api_key,
        llm_model: req.llm_model,
    };

    let sandbox_id = manager.create(config, req.timeout).await.map_err(|e| {
        error!(error = %e, "failed to create sandbox");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("failed to create sandbox: {e}"),
            }),
        )
    })?;

    let info = manager.get(&sandbox_id).await.ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "sandbox created but not found".to_string(),
            }),
        )
    })?;

    info!(sandbox = sandbox_id, "sandbox created via API");

    Ok((StatusCode::CREATED, Json(create_response_from_info(&info))))
}

/// `GET /api/v1/hiveboxes` — List all sandboxes.
pub async fn list_sandboxes(State(state): State<AppState>) -> Json<ListSandboxesResponse> {
    let manager = &state.manager;
    let sandboxes = manager.list().await;
    let total = sandboxes.len();

    let summaries: Vec<SandboxSummary> = sandboxes
        .into_iter()
        .map(|s| {
            let opencode_url = s
                .opencode_port
                .map(|_| format!("/api/v1/hiveboxes/{}/opencode/", s.id));
            SandboxSummary {
                id: s.id,
                status: format!("{:?}", s.state).to_lowercase(),
                image: s.image,
                uptime_seconds: s.uptime_seconds,
                ttl_seconds: s.ttl_seconds,
                memory: s.memory_limit.clone(),
                cpus: s.cpu_limit,
                commands_executed: s.commands_executed,
                network: s.network_mode.clone(),
                memory_usage_bytes: s.memory_usage_bytes,
                pid_current: s.pid_current,
                cpu_usage_usec: s.cpu_usage_usec,
                opencode_url,
            }
        })
        .collect();

    Json(ListSandboxesResponse {
        sandboxes: summaries,
        total,
    })
}

/// `GET /api/v1/hiveboxes/:id` — Get sandbox details.
pub async fn get_sandbox(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<SandboxStatusResponse>, (StatusCode, Json<ErrorResponse>)> {
    let manager = &state.manager;
    let info = manager.get(&id).await.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("sandbox '{id}' not found"),
            }),
        )
    })?;

    Ok(Json(SandboxStatusResponse {
        id: info.id,
        status: format!("{:?}", info.state).to_lowercase(),
        image: info.image,
        created_at: info.created_at.clone(),
        uptime_seconds: info.uptime_seconds,
        network: NetworkInfoResponse {
            mode: info.network_mode.clone(),
            ip: info.network_ip.clone(),
        },
        limits: LimitsResponse {
            memory: info.memory_limit.clone(),
            cpus: info.cpu_limit,
            pids: info.pid_limit,
        },
    }))
}

/// `POST /api/v1/hiveboxes/:id/exec` — Execute a command in a sandbox.
pub async fn exec_in_sandbox(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ExecRequest>,
) -> Result<Json<ExecResponse>, (StatusCode, Json<ErrorResponse>)> {
    let manager = &state.manager;
    let result = manager.exec(&id, &req.command).await.map_err(|e| {
        error!(sandbox = id, error = %e, "exec failed");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("exec failed: {e}"),
            }),
        )
    })?;

    Ok(Json(ExecResponse {
        exit_code: result.exit_code,
        stdout: result.stdout,
        stderr: result.stderr,
        duration_ms: result.duration_ms,
    }))
}

/// `DELETE /api/v1/hiveboxes/:id` — Destroy a sandbox.
pub async fn destroy_sandbox(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<DestroySandboxResponse>, (StatusCode, Json<ErrorResponse>)> {
    let manager = &state.manager;
    manager.destroy(&id).await.map_err(|e| {
        error!(sandbox = id, error = %e, "destroy failed");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("failed to destroy sandbox: {e}"),
            }),
        )
    })?;

    info!(sandbox = id, "sandbox destroyed via API");

    Ok(Json(DestroySandboxResponse {
        id,
        status: "destroyed".to_string(),
    }))
}

/// `PUT /api/v1/hiveboxes/:id/files` — Upload a file to a sandbox.
pub async fn upload_file(
    State(state): State<AppState>,
    Path(id): Path<String>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
    body: axum::body::Bytes,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    let manager = &state.manager;
    let path = params.get("path").ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "missing 'path' query parameter".to_string(),
            }),
        )
    })?;

    manager.write_file(&id, path, &body).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("failed to write file: {e}"),
            }),
        )
    })?;

    Ok(StatusCode::OK)
}

/// `GET /api/v1/hiveboxes/:id/files` — Download a file from a sandbox.
pub async fn download_file(
    State(state): State<AppState>,
    Path(id): Path<String>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> Result<Vec<u8>, (StatusCode, Json<ErrorResponse>)> {
    let manager = &state.manager;
    let path = params.get("path").ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "missing 'path' query parameter".to_string(),
            }),
        )
    })?;

    manager.read_file(&id, path).await.map_err(|e| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("failed to read file: {e}"),
            }),
        )
    })
}

/// `GET /api/v1/hiveboxes/:id/files/list` — List files and directories in a sandbox.
pub async fn list_files(
    State(state): State<AppState>,
    Path(id): Path<String>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> Result<Json<ListFilesResponse>, (StatusCode, Json<ErrorResponse>)> {
    let manager = &state.manager;
    let path = params.get("path").map(|s| s.as_str()).unwrap_or("/");

    let entries = manager.list_files(&id, path).await.map_err(|e| {
        let status = if e.to_string().contains("not found") {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        };
        (
            status,
            Json(ErrorResponse {
                error: format!("{e}"),
            }),
        )
    })?;

    let total = entries.len();
    Ok(Json(ListFilesResponse {
        path: path.to_string(),
        entries,
        total,
    }))
}

/// `GET /api/v1/analytics` — Get metrics history.
///
/// Query params:
/// - `range`: time range in seconds (e.g., 300 = 5min, 3600 = 1h). Default: all.
pub async fn get_analytics(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> Json<AnalyticsHistory> {
    let range_secs = params.get("range").and_then(|r| r.parse::<u64>().ok());
    Json(state.manager.get_analytics(range_secs).await)
}

/// Helper: build a CreateSandboxResponse from SandboxInfo.
fn create_response_from_info(info: &SandboxInfo) -> CreateSandboxResponse {
    let opencode_url = info
        .opencode_port
        .map(|_| format!("/api/v1/hiveboxes/{}/opencode/", info.id));
    CreateSandboxResponse {
        id: info.id.clone(),
        status: format!("{:?}", info.state).to_lowercase(),
        image: info.image.clone(),
        created_at: info.created_at.clone(),
        network: NetworkInfoResponse {
            mode: info.network_mode.clone(),
            ip: info.network_ip.clone(),
        },
        limits: LimitsResponse {
            memory: info.memory_limit.clone(),
            cpus: info.cpu_limit,
            pids: info.pid_limit,
        },
        expires_at: info.expires_at.clone(),
        opencode_url,
    }
}
