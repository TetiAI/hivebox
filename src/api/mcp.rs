//! HTTP-based MCP endpoint for remote clients.
//!
//! Implements MCP (Model Context Protocol) over Streamable HTTP transport.
//! Clients send JSON-RPC requests via POST and receive JSON-RPC responses.
//!
//! Endpoint: `POST /api/v1/hiveboxes/:id/mcp`
//!
//! This allows remote MCP clients to connect without needing the `hivebox`
//! binary locally — just a URL and an API key.
//!
//! ```json
//! {
//!   "mcpServers": {
//!     "hivebox": {
//!       "url": "http://your-server:7070/api/v1/hiveboxes/my-sandbox/mcp",
//!       "headers": { "Authorization": "Bearer your-key" }
//!     }
//!   }
//! }
//! ```

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};
use tracing::debug;

use super::handlers::AppState;
use crate::sandbox::manager::SandboxManager;

// ── JSON-RPC types ──────────────────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<serde_json::Value>,
    method: String,
    #[serde(default)]
    params: Option<serde_json::Value>,
}

#[derive(Serialize)]
pub(crate) struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Serialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

impl JsonRpcResponse {
    fn success(id: serde_json::Value, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    fn error(id: serde_json::Value, code: i64, message: String) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError { code, message }),
        }
    }
}

// ── HTTP MCP handler ────────────────────────────────────────────────────

/// `POST /api/v1/hiveboxes/:id/mcp` — MCP over HTTP.
pub async fn mcp_handler(
    State(state): State<AppState>,
    Path(sandbox_id): Path<String>,
    req_parts: axum::extract::Request,
) -> (StatusCode, Json<JsonRpcResponse>) {
    let body = axum::body::to_bytes(req_parts.into_body(), 10 * 1024 * 1024).await;
    let req: JsonRpcRequest = match body {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(r) => r,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(JsonRpcResponse::error(
                        serde_json::Value::Null,
                        -32700,
                        format!("parse error: {e}"),
                    )),
                );
            }
        },
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(JsonRpcResponse::error(
                    serde_json::Value::Null,
                    -32700,
                    format!("body error: {e}"),
                )),
            );
        }
    };

    let id = match req.id {
        Some(id) => id,
        None => {
            return (
                StatusCode::NO_CONTENT,
                Json(JsonRpcResponse::success(
                    serde_json::Value::Null,
                    serde_json::json!(null),
                )),
            );
        }
    };

    let response = match req.method.as_str() {
        "initialize" => JsonRpcResponse::success(
            id,
            serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": "hivebox",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "instructions": concat!(
                    "HiveBox is a lightweight Linux sandbox. ",
                    "The sandbox runs Alpine Linux (musl libc, apk package manager). ",
                    "Use 'apk add <pkg>' to install packages (e.g. apk add python3, apk add nodejs npm, apk add git). ",
                    "The default working directory is /workspace. ",
                    "Commands run as root inside the sandbox. ",
                    "Use the 'exec' tool for shell commands and the file tools for reading/writing files."
                )
            }),
        ),
        "tools/list" => JsonRpcResponse::success(
            id,
            serde_json::json!({
                "tools": crate::mcp::tool_definitions()
            }),
        ),
        "tools/call" => {
            let params = req.params.unwrap_or_default();
            let name = params["name"].as_str().unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or_default();

            debug!(
                tool = name,
                sandbox = sandbox_id,
                "executing MCP tool via HTTP"
            );
            let result = handle_tool_call(&state.manager, &sandbox_id, name, &args).await;
            JsonRpcResponse::success(id, result)
        }
        "ping" => JsonRpcResponse::success(id, serde_json::json!({})),
        _ => JsonRpcResponse::error(id, -32601, format!("method not found: {}", req.method)),
    };

    (StatusCode::OK, Json(response))
}

// ── Tool dispatch (calls SandboxManager directly) ───────────────────────

async fn handle_tool_call(
    manager: &Arc<SandboxManager>,
    sandbox_id: &str,
    name: &str,
    args: &serde_json::Value,
) -> serde_json::Value {
    let result = match name {
        "exec" => tool_exec(manager, sandbox_id, args).await,
        "read_file" => tool_read_file(manager, sandbox_id, args).await,
        "read_multiple_files" => tool_read_multiple_files(manager, sandbox_id, args).await,
        "write_file" => tool_write_file(manager, sandbox_id, args).await,
        "edit_file" => tool_edit_file(manager, sandbox_id, args).await,
        "list_directory" => tool_list_directory(manager, sandbox_id, args).await,
        "directory_tree" => tool_directory_tree(manager, sandbox_id, args).await,
        "search_files" => tool_search_files(manager, sandbox_id, args).await,
        "get_file_info" => tool_get_file_info(manager, sandbox_id, args).await,
        "create_directory" => tool_create_directory(manager, sandbox_id, args).await,
        "move_file" => tool_move_file(manager, sandbox_id, args).await,
        "read_media_file" => tool_read_media_file(manager, sandbox_id, args).await,
        "list_directory_with_sizes" => {
            tool_list_directory_with_sizes(manager, sandbox_id, args).await
        }
        "glob" => tool_glob(manager, sandbox_id, args).await,
        _ => Err(anyhow::anyhow!("unknown tool: {name}")),
    };

    match result {
        Ok(text) => serde_json::json!({
            "content": [{ "type": "text", "text": text }]
        }),
        Err(e) => serde_json::json!({
            "content": [{ "type": "text", "text": format!("Error: {e:#}") }],
            "isError": true
        }),
    }
}

// ── Helper: exec and format ─────────────────────────────────────────────

async fn exec_cmd(
    manager: &Arc<SandboxManager>,
    sandbox_id: &str,
    command: &str,
) -> anyhow::Result<crate::runtime::ExecResult> {
    manager.exec(sandbox_id, command).await
}

// ── Tool implementations (direct SandboxManager calls) ──────────────────

async fn tool_exec(
    manager: &Arc<SandboxManager>,
    sandbox_id: &str,
    args: &serde_json::Value,
) -> anyhow::Result<String> {
    let command = args["command"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'command'"))?;
    let result = exec_cmd(manager, sandbox_id, command).await?;
    let mut output = result.stdout;
    if !result.stderr.is_empty() {
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str("[stderr]\n");
        output.push_str(&result.stderr);
    }
    Ok(output)
}

async fn tool_read_file(
    manager: &Arc<SandboxManager>,
    sandbox_id: &str,
    args: &serde_json::Value,
) -> anyhow::Result<String> {
    let path = args["path"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'path'"))?;
    let head = args.get("head").and_then(|v| v.as_u64());
    let tail = args.get("tail").and_then(|v| v.as_u64());

    let cmd = if let Some(n) = head {
        format!("head -n {n} '{path}'")
    } else if let Some(n) = tail {
        format!("tail -n {n} '{path}'")
    } else {
        format!("cat -n '{path}'")
    };

    let result = exec_cmd(manager, sandbox_id, &cmd).await?;
    if result.exit_code != 0 && !result.stderr.is_empty() {
        anyhow::bail!("{}", result.stderr.trim());
    }
    Ok(result.stdout)
}

async fn tool_read_multiple_files(
    manager: &Arc<SandboxManager>,
    sandbox_id: &str,
    args: &serde_json::Value,
) -> anyhow::Result<String> {
    let paths = args["paths"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("missing 'paths'"))?;
    let mut output = String::new();

    for path_val in paths {
        let path = path_val.as_str().unwrap_or("");
        if !output.is_empty() {
            output.push_str("\n---\n");
        }
        output.push_str(&format!("=== {path} ===\n"));

        let result = exec_cmd(manager, sandbox_id, &format!("cat -n '{path}'")).await?;
        if result.exit_code != 0 {
            output.push_str(&format!("[error] {}\n", result.stderr.trim()));
        } else {
            output.push_str(&result.stdout);
        }
    }

    Ok(output)
}

async fn tool_write_file(
    manager: &Arc<SandboxManager>,
    sandbox_id: &str,
    args: &serde_json::Value,
) -> anyhow::Result<String> {
    let path = args["path"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'path'"))?;
    let content = args["content"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'content'"))?;

    manager
        .write_file(sandbox_id, path, content.as_bytes())
        .await?;
    Ok(format!("Written to {path} ({} bytes)", content.len()))
}

async fn tool_edit_file(
    manager: &Arc<SandboxManager>,
    sandbox_id: &str,
    args: &serde_json::Value,
) -> anyhow::Result<String> {
    let path = args["path"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'path'"))?;
    let old_text = args["old_text"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'old_text'"))?;
    let new_text = args["new_text"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'new_text'"))?;

    let content_bytes = manager.read_file(sandbox_id, path).await?;
    let content =
        String::from_utf8(content_bytes).map_err(|_| anyhow::anyhow!("file is not valid UTF-8"))?;

    let count = content.matches(old_text).count();
    if count == 0 {
        anyhow::bail!("old_text not found in {path}");
    }
    if count > 1 {
        anyhow::bail!("old_text matches {count} times in {path} — must be unique");
    }

    let new_content = content.replacen(old_text, new_text, 1);
    manager
        .write_file(sandbox_id, path, new_content.as_bytes())
        .await?;
    Ok(format!("Edited {path} (1 replacement)"))
}

async fn tool_list_directory(
    manager: &Arc<SandboxManager>,
    sandbox_id: &str,
    args: &serde_json::Value,
) -> anyhow::Result<String> {
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let result = exec_cmd(manager, sandbox_id, &format!("ls -la '{path}'")).await?;
    Ok(result.stdout)
}

async fn tool_directory_tree(
    manager: &Arc<SandboxManager>,
    sandbox_id: &str,
    args: &serde_json::Value,
) -> anyhow::Result<String> {
    let path = args["path"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'path'"))?;
    let max_depth = args.get("max_depth").and_then(|v| v.as_u64()).unwrap_or(3);
    let result = exec_cmd(
        manager,
        sandbox_id,
        &format!("find '{path}' -maxdepth {max_depth} -not -path '*/\\.*' | head -200 | sort"),
    )
    .await?;
    Ok(result.stdout)
}

async fn tool_search_files(
    manager: &Arc<SandboxManager>,
    sandbox_id: &str,
    args: &serde_json::Value,
) -> anyhow::Result<String> {
    let path = args["path"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'path'"))?;
    let pattern = args["pattern"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'pattern'"))?;
    let file_pattern = args.get("file_pattern").and_then(|v| v.as_str());

    let include = file_pattern
        .map(|p| format!(" --include='{p}'"))
        .unwrap_or_default();
    let result = exec_cmd(
        manager,
        sandbox_id,
        &format!("grep -rn '{pattern}' '{path}'{include} | head -100"),
    )
    .await?;
    Ok(result.stdout)
}

async fn tool_get_file_info(
    manager: &Arc<SandboxManager>,
    sandbox_id: &str,
    args: &serde_json::Value,
) -> anyhow::Result<String> {
    let path = args["path"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'path'"))?;
    let result = exec_cmd(manager, sandbox_id, &format!("stat '{path}'")).await?;
    Ok(result.stdout)
}

async fn tool_create_directory(
    manager: &Arc<SandboxManager>,
    sandbox_id: &str,
    args: &serde_json::Value,
) -> anyhow::Result<String> {
    let path = args["path"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'path'"))?;
    let result = exec_cmd(manager, sandbox_id, &format!("mkdir -p '{path}'")).await?;
    if result.exit_code != 0 {
        anyhow::bail!("{}", result.stderr.trim());
    }
    Ok(format!("Created directory {path}"))
}

async fn tool_move_file(
    manager: &Arc<SandboxManager>,
    sandbox_id: &str,
    args: &serde_json::Value,
) -> anyhow::Result<String> {
    let source = args["source"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'source'"))?;
    let destination = args["destination"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'destination'"))?;
    let result = exec_cmd(
        manager,
        sandbox_id,
        &format!("mv '{source}' '{destination}'"),
    )
    .await?;
    if result.exit_code != 0 {
        anyhow::bail!("{}", result.stderr.trim());
    }
    Ok(format!("Moved {source} → {destination}"))
}

async fn tool_read_media_file(
    manager: &Arc<SandboxManager>,
    sandbox_id: &str,
    args: &serde_json::Value,
) -> anyhow::Result<String> {
    let path = args["path"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'path'"))?;

    let content = manager.read_file(sandbox_id, path).await?;
    let b64 = BASE64.encode(&content);

    let mime = match path
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    };

    Ok(format!("data:{mime};base64,{b64}"))
}

async fn tool_list_directory_with_sizes(
    manager: &Arc<SandboxManager>,
    sandbox_id: &str,
    args: &serde_json::Value,
) -> anyhow::Result<String> {
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let result = exec_cmd(manager, sandbox_id, &format!("ls -lhS '{path}'")).await?;
    Ok(result.stdout)
}

async fn tool_glob(
    manager: &Arc<SandboxManager>,
    sandbox_id: &str,
    args: &serde_json::Value,
) -> anyhow::Result<String> {
    let pattern = args["pattern"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'pattern'"))?;
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or("/workspace");

    let cmd = if pattern.contains('/') {
        format!(
            "cd '{path}' && find . -path './{pattern}' -not -path '*/\\.*' 2>/dev/null | head -500 | sed 's|^\\./||' | sort"
        )
    } else {
        format!(
            "cd '{path}' && find . -name '{pattern}' -not -path '*/\\.*' 2>/dev/null | head -500 | sed 's|^\\./||' | sort"
        )
    };

    let result = exec_cmd(manager, sandbox_id, &cmd).await?;
    if result.stdout.trim().is_empty() {
        Ok("No files matched the pattern.".to_string())
    } else {
        Ok(result.stdout)
    }
}

