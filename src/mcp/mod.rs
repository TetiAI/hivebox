//! MCP (Model Context Protocol) stdio server for HiveBox sandboxes.
//!
//! Exposes sandbox operations as MCP tools over stdin/stdout (JSON-RPC 2.0).
//! Designed to be spawned by OpenCode, Claude Code, or any MCP-compatible client:
//!
//! ```json
//! {
//!   "mcpServers": {
//!     "sandbox": {
//!       "command": "hivebox",
//!       "args": ["mcp", "--sandbox", "abc123", "--api-url", "http://localhost:7070"]
//!     }
//!   }
//! }
//! ```
//!
//! Each tool call is translated into an HTTP request to the HiveBox daemon,
//! which executes it inside the sandbox via nsenter.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::{debug, error};

// ── JSON-RPC 2.0 types ─────────────────────────────────────────────────

#[derive(Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<serde_json::Value>,
    method: String,
    #[serde(default)]
    params: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct JsonRpcResponse {
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

// ── HiveBox daemon API client ───────────────────────────────────────────

/// HTTP client wrapper for the HiveBox daemon API.
struct HiveboxClient {
    sandbox_id: String,
    api_url: String,
    api_key: Option<String>,
    client: reqwest::Client,
}

#[derive(Deserialize)]
struct ExecResponse {
    #[allow(dead_code)]
    exit_code: i32,
    stdout: String,
    stderr: String,
    #[allow(dead_code)]
    duration_ms: u64,
}

impl HiveboxClient {
    fn new(sandbox_id: String, api_url: String, api_key: Option<String>) -> Self {
        Self {
            sandbox_id,
            api_url,
            api_key,
            client: reqwest::Client::new(),
        }
    }

    /// Execute a shell command inside the sandbox.
    async fn exec(&self, command: &str) -> Result<ExecResponse> {
        let url = format!("{}/api/v1/hiveboxes/{}/exec", self.api_url, self.sandbox_id);
        let mut req = self
            .client
            .post(&url)
            .json(&serde_json::json!({ "command": command }));
        if let Some(ref key) = self.api_key {
            req = req.header("Authorization", format!("Bearer {key}"));
        }
        let resp = req.send().await.context("failed to reach HiveBox daemon")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("daemon returned {status}: {body}");
        }
        resp.json().await.context("failed to parse exec response")
    }

    /// Write a file into the sandbox.
    async fn write_file(&self, path: &str, content: &[u8]) -> Result<()> {
        let url = format!(
            "{}/api/v1/hiveboxes/{}/files?path={}",
            self.api_url,
            self.sandbox_id,
            urlencoded(path)
        );
        let mut req = self
            .client
            .put(&url)
            .header("Content-Type", "application/octet-stream")
            .body(content.to_vec());
        if let Some(ref key) = self.api_key {
            req = req.header("Authorization", format!("Bearer {key}"));
        }
        let resp = req.send().await.context("failed to reach HiveBox daemon")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("write_file failed ({status}): {body}");
        }
        Ok(())
    }

    /// Read a file from the sandbox.
    async fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        let url = format!(
            "{}/api/v1/hiveboxes/{}/files?path={}",
            self.api_url,
            self.sandbox_id,
            urlencoded(path)
        );
        let mut req = self.client.get(&url);
        if let Some(ref key) = self.api_key {
            req = req.header("Authorization", format!("Bearer {key}"));
        }
        let resp = req.send().await.context("failed to reach HiveBox daemon")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("read_file failed ({status}): {body}");
        }
        Ok(resp.bytes().await?.to_vec())
    }
}

/// Builds MCP instructions with pre-installed package info from env vars.
fn build_mcp_instructions() -> String {
    let mut s = String::from(
        "HiveBox is a lightweight Linux sandbox running Alpine Linux (musl libc, apk package manager). \
         Commands run as root. The default working directory is /. ",
    );
    if let Ok(pkgs) = std::env::var("HIVEBOX_PACKAGES") {
        s.push_str(&format!(
            "Pre-installed system packages (DO NOT reinstall): {}. ",
            pkgs
        ));
    }
    if let Ok(pkgs) = std::env::var("HIVEBOX_PIP_PACKAGES") {
        s.push_str(&format!("Pre-installed pip packages: {}. ", pkgs));
    }
    if let Ok(pkgs) = std::env::var("HIVEBOX_NPM_PACKAGES") {
        s.push_str(&format!("Pre-installed npm global packages: {}. ", pkgs));
    }
    s.push_str(
        "When installing pip packages, always use: pip install --break-system-packages <pkg>. ",
    );
    s.push_str(
        "Use the 'exec' tool for shell commands and the file tools for reading/writing files.",
    );
    s
}

/// Simple percent-encoding for path query parameters.
fn urlencoded(s: &str) -> String {
    s.replace('%', "%25")
        .replace(' ', "%20")
        .replace('#', "%23")
        .replace('&', "%26")
        .replace('?', "%3F")
        .replace('=', "%3D")
}

// ── MCP tool definitions ────────────────────────────────────────────────

pub fn tool_definitions() -> serde_json::Value {
    serde_json::json!([
        {
            "name": "exec",
            "description": "Execute a shell command in the sandbox. Returns stdout, stderr, and exit code.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Shell command to execute (passed to /bin/sh -c)"
                    }
                },
                "required": ["command"]
            }
        },
        {
            "name": "read_file",
            "description": "Read the contents of a file. Optionally read only the first or last N lines.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path (absolute or relative to /)" },
                    "head": { "type": "integer", "description": "Read only the first N lines" },
                    "tail": { "type": "integer", "description": "Read only the last N lines" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "read_multiple_files",
            "description": "Read multiple files at once. Returns contents with clear separators.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "paths": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "List of file paths to read"
                    }
                },
                "required": ["paths"]
            }
        },
        {
            "name": "write_file",
            "description": "Create or overwrite a file with the given content.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path to write to" },
                    "content": { "type": "string", "description": "Content to write" }
                },
                "required": ["path", "content"]
            }
        },
        {
            "name": "edit_file",
            "description": "Make a targeted edit by replacing exact text. old_text must match exactly (including whitespace).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path to edit" },
                    "old_text": { "type": "string", "description": "Exact text to find and replace" },
                    "new_text": { "type": "string", "description": "Replacement text" }
                },
                "required": ["path", "old_text", "new_text"]
            }
        },
        {
            "name": "list_directory",
            "description": "List directory contents with file types, permissions, and sizes.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Directory path (defaults to current directory)" }
                }
            }
        },
        {
            "name": "directory_tree",
            "description": "Get a recursive tree view of a directory structure.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Root directory path" },
                    "max_depth": { "type": "integer", "description": "Maximum depth (default: 3)" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "search_files",
            "description": "Search for text patterns in files recursively. Returns matching lines with paths and line numbers.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Directory to search in" },
                    "pattern": { "type": "string", "description": "Text or regex pattern to search for" },
                    "file_pattern": { "type": "string", "description": "Glob pattern to filter files (e.g. '*.py')" }
                },
                "required": ["path", "pattern"]
            }
        },
        {
            "name": "get_file_info",
            "description": "Get file metadata (size, permissions, modification time, type).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File or directory path" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "create_directory",
            "description": "Create a directory and any necessary parent directories.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Directory path to create" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "move_file",
            "description": "Move or rename a file or directory.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "source": { "type": "string", "description": "Source path" },
                    "destination": { "type": "string", "description": "Destination path" }
                },
                "required": ["source", "destination"]
            }
        },
        {
            "name": "read_media_file",
            "description": "Read an image or media file as base64-encoded content with MIME type. Useful for images (PNG, JPG, GIF, WebP), audio (MP3, WAV), video, and PDFs.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the media file" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "list_directory_with_sizes",
            "description": "List directory contents with human-readable file sizes, sorted by size (largest first).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Directory path (defaults to current directory)" }
                }
            }
        },
        {
            "name": "glob",
            "description": "Find files matching glob patterns. Returns a list of matching file paths. Respects .gitignore if git is available.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Glob pattern to match (e.g. '**/*.ts', 'src/**/*.rs', '*.json')" },
                    "path": { "type": "string", "description": "Root directory to search in (defaults to /)" }
                },
                "required": ["pattern"]
            }
        },
        {
            "name": "list_skills",
            "description": "List all available skills with their names and descriptions. Call this first to discover what skills are available before using one.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        },
        {
            "name": "read_skill_file",
            "description": "Read a file from a skill directory (e.g. SKILL.md for instructions, or a reference file like pptxgenjs.md). Always read SKILL.md first to understand how to use the skill.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "skill": { "type": "string", "description": "Skill name (e.g. 'pptx', 'pdf', 'docx')" },
                    "file": { "type": "string", "description": "File to read (defaults to SKILL.md)" }
                },
                "required": ["skill"]
            }
        }
    ])
}

// ── Tool execution ──────────────────────────────────────────────────────

/// Dispatches an MCP tools/call request to the appropriate handler.
async fn handle_tool_call(
    client: &HiveboxClient,
    skills_path: &Path,
    name: &str,
    args: &serde_json::Value,
) -> serde_json::Value {
    let result = match name {
        "exec" => tool_exec(client, args).await,
        "read_file" => tool_read_file(client, args).await,
        "read_multiple_files" => tool_read_multiple_files(client, args).await,
        "write_file" => tool_write_file(client, args).await,
        "edit_file" => tool_edit_file(client, args).await,
        "list_directory" => tool_list_directory(client, args).await,
        "directory_tree" => tool_directory_tree(client, args).await,
        "search_files" => tool_search_files(client, args).await,
        "get_file_info" => tool_get_file_info(client, args).await,
        "create_directory" => tool_create_directory(client, args).await,
        "move_file" => tool_move_file(client, args).await,
        "read_media_file" => tool_read_media_file(client, args).await,
        "list_directory_with_sizes" => tool_list_directory_with_sizes(client, args).await,
        "glob" => tool_glob(client, args).await,
        "list_skills" => tool_list_skills(skills_path).await,
        "read_skill_file" => tool_read_skill_file(skills_path, args).await,
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

async fn tool_exec(client: &HiveboxClient, args: &serde_json::Value) -> Result<String> {
    let command = args["command"].as_str().context("missing 'command'")?;
    let resp = client.exec(command).await?;
    let mut output = resp.stdout;
    if !resp.stderr.is_empty() {
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str("[stderr]\n");
        output.push_str(&resp.stderr);
    }
    Ok(output)
}

async fn tool_read_file(client: &HiveboxClient, args: &serde_json::Value) -> Result<String> {
    let path = args["path"].as_str().context("missing 'path'")?;
    let head = args.get("head").and_then(|v| v.as_u64());
    let tail = args.get("tail").and_then(|v| v.as_u64());

    let cmd = if let Some(n) = head {
        format!("head -n {n} '{path}'")
    } else if let Some(n) = tail {
        format!("tail -n {n} '{path}'")
    } else {
        format!("cat -n '{path}'")
    };

    let resp = client.exec(&cmd).await?;
    if resp.exit_code != 0 && !resp.stderr.is_empty() {
        anyhow::bail!("{}", resp.stderr.trim());
    }
    Ok(resp.stdout)
}

async fn tool_read_multiple_files(
    client: &HiveboxClient,
    args: &serde_json::Value,
) -> Result<String> {
    let paths = args["paths"].as_array().context("missing 'paths'")?;
    let mut output = String::new();

    for path_val in paths {
        let path = path_val.as_str().unwrap_or("");
        if !output.is_empty() {
            output.push_str("\n---\n");
        }
        output.push_str(&format!("=== {path} ===\n"));

        let resp = client.exec(&format!("cat -n '{path}'")).await?;
        if resp.exit_code != 0 {
            output.push_str(&format!("[error] {}\n", resp.stderr.trim()));
        } else {
            output.push_str(&resp.stdout);
        }
    }

    Ok(output)
}

async fn tool_write_file(client: &HiveboxClient, args: &serde_json::Value) -> Result<String> {
    let path = args["path"].as_str().context("missing 'path'")?;
    let content = args["content"].as_str().context("missing 'content'")?;

    client.write_file(path, content.as_bytes()).await?;
    Ok(format!("Written to {path} ({} bytes)", content.len()))
}

async fn tool_edit_file(client: &HiveboxClient, args: &serde_json::Value) -> Result<String> {
    let path = args["path"].as_str().context("missing 'path'")?;
    let old_text = args["old_text"].as_str().context("missing 'old_text'")?;
    let new_text = args["new_text"].as_str().context("missing 'new_text'")?;

    // Read file via API, replace, write back.
    let content_bytes = client.read_file(path).await?;
    let content = String::from_utf8(content_bytes).context("file is not valid UTF-8")?;

    let count = content.matches(old_text).count();
    if count == 0 {
        anyhow::bail!("old_text not found in {path}");
    }
    if count > 1 {
        anyhow::bail!("old_text matches {count} times in {path} — must be unique");
    }

    let new_content = content.replacen(old_text, new_text, 1);
    client.write_file(path, new_content.as_bytes()).await?;
    Ok(format!("Edited {path} (1 replacement)"))
}

async fn tool_list_directory(client: &HiveboxClient, args: &serde_json::Value) -> Result<String> {
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let resp = client.exec(&format!("ls -la '{path}'")).await?;
    Ok(resp.stdout)
}

async fn tool_directory_tree(client: &HiveboxClient, args: &serde_json::Value) -> Result<String> {
    let path = args["path"].as_str().context("missing 'path'")?;
    let max_depth = args.get("max_depth").and_then(|v| v.as_u64()).unwrap_or(3);
    let resp = client
        .exec(&format!(
            "find '{path}' -maxdepth {max_depth} -not -path '*/\\.*' | head -200 | sort"
        ))
        .await?;
    Ok(resp.stdout)
}

async fn tool_search_files(client: &HiveboxClient, args: &serde_json::Value) -> Result<String> {
    let path = args["path"].as_str().context("missing 'path'")?;
    let pattern = args["pattern"].as_str().context("missing 'pattern'")?;
    let file_pattern = args.get("file_pattern").and_then(|v| v.as_str());

    let include = file_pattern
        .map(|p| format!(" --include='{p}'"))
        .unwrap_or_default();
    let resp = client
        .exec(&format!(
            "grep -rn '{pattern}' '{path}'{include} | head -100"
        ))
        .await?;
    Ok(resp.stdout)
}

async fn tool_get_file_info(client: &HiveboxClient, args: &serde_json::Value) -> Result<String> {
    let path = args["path"].as_str().context("missing 'path'")?;
    let resp = client.exec(&format!("stat '{path}'")).await?;
    Ok(resp.stdout)
}

async fn tool_create_directory(client: &HiveboxClient, args: &serde_json::Value) -> Result<String> {
    let path = args["path"].as_str().context("missing 'path'")?;
    let resp = client.exec(&format!("mkdir -p '{path}'")).await?;
    if resp.exit_code != 0 {
        anyhow::bail!("{}", resp.stderr.trim());
    }
    Ok(format!("Created directory {path}"))
}

async fn tool_move_file(client: &HiveboxClient, args: &serde_json::Value) -> Result<String> {
    let source = args["source"].as_str().context("missing 'source'")?;
    let destination = args["destination"]
        .as_str()
        .context("missing 'destination'")?;
    let resp = client
        .exec(&format!("mv '{source}' '{destination}'"))
        .await?;
    if resp.exit_code != 0 {
        anyhow::bail!("{}", resp.stderr.trim());
    }
    Ok(format!("Moved {source} → {destination}"))
}

async fn tool_read_media_file(client: &HiveboxClient, args: &serde_json::Value) -> Result<String> {
    let path = args["path"].as_str().context("missing 'path'")?;
    let content = client.read_file(path).await?;
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
    client: &HiveboxClient,
    args: &serde_json::Value,
) -> Result<String> {
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let resp = client.exec(&format!("ls -lhS '{path}'")).await?;
    Ok(resp.stdout)
}

async fn tool_glob(client: &HiveboxClient, args: &serde_json::Value) -> Result<String> {
    let pattern = args["pattern"].as_str().context("missing 'pattern'")?;
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("/");

    // Use find with -name/-path depending on whether pattern contains /
    let cmd = if pattern.contains('/') {
        // Pattern with path separators: use find -path
        format!(
            "cd '{path}' && find . -path './{pattern}' -not -path '*/\\.*' 2>/dev/null | head -500 | sed 's|^\\./||' | sort"
        )
    } else {
        // Simple filename pattern: use find -name
        format!(
            "cd '{path}' && find . -name '{pattern}' -not -path '*/\\.*' 2>/dev/null | head -500 | sed 's|^\\./||' | sort"
        )
    };

    let resp = client.exec(&cmd).await?;
    if resp.stdout.trim().is_empty() {
        Ok("No files matched the pattern.".to_string())
    } else {
        Ok(resp.stdout)
    }
}

// ── MCP server main loop ────────────────────────────────────────────────

async fn tool_list_skills(skills_path: &Path) -> Result<String> {
    if !skills_path.exists() {
        return Ok(format!(
            "Skills directory not found: {}",
            skills_path.display()
        ));
    }
    let mut skills = Vec::new();
    for entry in std::fs::read_dir(skills_path)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let skill_md = entry.path().join("SKILL.md");
        let description = std::fs::read_to_string(&skill_md)
            .ok()
            .and_then(|s| {
                s.lines()
                    .find(|l| l.starts_with("# "))
                    .map(|l| l.trim_start_matches("# ").to_string())
            })
            .unwrap_or_default();
        skills.push(format!("- **{name}**: {description}"));
    }
    skills.sort();
    if skills.is_empty() {
        return Ok("No skills available.".to_string());
    }
    Ok(format!(
        "Available skills:\n\n{}\n\nUse read_skill_file to read a skill's instructions.",
        skills.join("\n")
    ))
}

async fn tool_read_skill_file(skills_path: &Path, args: &serde_json::Value) -> Result<String> {
    let skill = args["skill"].as_str().context("missing 'skill'")?;
    let file = args
        .get("file")
        .and_then(|v| v.as_str())
        .unwrap_or("SKILL.md");

    if skill.contains("..") || skill.contains('/') || skill.contains('\\') {
        anyhow::bail!("invalid skill name: {skill}");
    }
    if file.contains("..") || file.starts_with('/') || file.starts_with('\\') {
        anyhow::bail!("invalid file name: {file}");
    }

    let path = skills_path.join(skill).join(file);
    if !path.exists() {
        anyhow::bail!("not found: {skill}/{file}");
    }
    std::fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))
}

/// Runs the MCP server over stdin/stdout (newline-delimited JSON-RPC 2.0).
pub async fn run(
    sandbox_id: String,
    api_url: String,
    api_key: Option<String>,
    skills_path: PathBuf,
) -> Result<()> {
    let client = HiveboxClient::new(sandbox_id.clone(), api_url, api_key);

    let stdin = tokio::io::stdin();
    let reader = BufReader::new(stdin);
    let mut lines = reader.lines();

    // Log to stderr only (stdout is the MCP transport).
    eprintln!("[hivebox-mcp] serving sandbox '{sandbox_id}' — waiting for requests on stdin");

    while let Some(line) = lines.next_line().await? {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let req: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                error!("invalid JSON-RPC: {e}");
                continue;
            }
        };

        // Notifications have no id — acknowledge silently.
        let id = match req.id {
            Some(id) => id,
            None => {
                debug!(method = req.method, "received notification");
                continue;
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
                    "instructions": build_mcp_instructions()
                }),
            ),
            "tools/list" => JsonRpcResponse::success(
                id,
                serde_json::json!({
                    "tools": tool_definitions()
                }),
            ),
            "tools/call" => {
                let params = req.params.unwrap_or_default();
                let name = params["name"].as_str().unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or_default();

                debug!(tool = name, "executing tool");
                let result = handle_tool_call(&client, &skills_path, name, &args).await;
                JsonRpcResponse::success(id, result)
            }
            "ping" => JsonRpcResponse::success(id, serde_json::json!({})),
            _ => JsonRpcResponse::error(id, -32601, format!("method not found: {}", req.method)),
        };

        // Write response to stdout (one JSON object per line).
        let out = serde_json::to_string(&response)?;
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(out.as_bytes())?;
        stdout.write_all(b"\n")?;
        stdout.flush()?;
    }

    eprintln!("[hivebox-mcp] stdin closed, shutting down");
    Ok(())
}
