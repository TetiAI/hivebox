# REST API Reference

Base URL: `http://<host>:7070`

## Authentication

All `/api/` endpoints require a Bearer token in the `Authorization` header:

```
Authorization: Bearer <your-api-key>
```

The `/healthz` and `/dashboard` endpoints do not require authentication.

Set the API key when starting the daemon:
```bash
hivebox daemon --api-key your-secret-key
# or
HIVEBOX_API_KEY=your-secret-key hivebox daemon
```

## Web Dashboard

```
GET /dashboard
```

A built-in management UI served directly by the HiveBox daemon. Provides sandbox creation, listing, terminal access, and destruction — all from the browser. Login with your API key.

No authentication required to load the page (authentication happens client-side via API calls).

## Endpoints

### Health Check

```
GET /healthz
```

Returns `ok` if the server is running. No authentication required.

**Response**: `200 OK` with body `ok`

---

### Create Sandbox

```
POST /api/v1/hiveboxes
```

Creates a new persistent sandbox.

**Request body**:
```json
{
  "name": "my-sandbox",
  "memory": "512m",
  "cpus": 2.0,
  "pids": 128,
  "network": "isolated",
  "timeout": 3600,
  "skills": ["pdf", "docx"],
  "custom_mcps": {
    "my-server": { "type": "remote", "url": "https://example.com/mcp", "enabled": true }
  },
  "llm_base_url": "https://api.openai.com/v1",
  "llm_api_key": "sk-...",
  "llm_model": "gpt-4o"
}
```

All fields are optional. Defaults:
| Field | Default | Description |
|-------|---------|-------------|
| `name` | random ID | Unique sandbox name |
| `memory` | `"512m"` | Memory limit |
| `cpus` | `1.0` | CPU fraction |
| `pids` | `128` | Max processes |
| `network` | `"none"` | Network mode: `none`, `isolated`, `shared` |
| `timeout` | `3600` | Auto-destroy timeout in seconds (max 86400) |
| `skills` | all defaults | Skills for the AI agent. Omit for all, `[]` for none, or list specific names |
| `custom_mcps` | none | Additional MCP servers (merged with built-in `hivebox` MCP) |
| `llm_base_url` | global default | LLM API base URL (overrides `HIVEBOX_OPENCODE_BASE_URL`) |
| `llm_api_key` | global default | LLM API key (overrides `HIVEBOX_OPENCODE_API_KEY`) |
| `llm_model` | global default | LLM model name (overrides `HIVEBOX_OPENCODE_MODEL`) |

**Response** (`201 Created`):
```json
{
  "id": "my-sandbox",
  "status": "running",
  "image": "base",
  "created_at": "2025-01-15T10:30:00Z",
  "network": {
    "mode": "isolated",
    "ip": "10.10.0.2"
  },
  "limits": {
    "memory": "512m",
    "cpus": 2.0,
    "pids": 128
  },
  "expires_at": "2025-01-15T11:30:00Z"
}
```

---

### List Sandboxes

```
GET /api/v1/hiveboxes
```

Returns all active sandboxes.

**Response** (`200 OK`):
```json
{
  "sandboxes": [
    {
      "id": "my-sandbox",
      "status": "running",
      "image": "base",
      "uptime_seconds": 120,
      "ttl_seconds": 3480,
      "memory": "512m",
      "cpus": 2.0,
      "commands_executed": 5,
      "network": "isolated"
    }
  ],
  "total": 1
}
```

---

### Get Sandbox Details

```
GET /api/v1/hiveboxes/:id
```

Returns detailed information about a sandbox.

**Response** (`200 OK`):
```json
{
  "id": "my-sandbox",
  "status": "running",
  "image": "base",
  "created_at": "2025-01-15T10:30:00Z",
  "uptime_seconds": 120,
  "network": {
    "mode": "isolated",
    "ip": "10.10.0.2"
  },
  "limits": {
    "memory": "512m",
    "cpus": 2.0,
    "pids": 128
  }
}
```

**Errors**: `404 Not Found` if sandbox doesn't exist.

---

### Execute Command

```
POST /api/v1/hiveboxes/:id/exec
```

Executes a command inside a running sandbox.

**Request body**:
```json
{
  "command": "python3 -c 'print(1+1)'",
  "timeout": 30
}
```

| Field | Default | Description |
|-------|---------|-------------|
| `command` | required | Shell command to execute |
| `timeout` | `0` | Command timeout in seconds |

**Response** (`200 OK`):
```json
{
  "exit_code": 0,
  "stdout": "2\n",
  "stderr": "",
  "duration_ms": 45
}
```

**Errors**: `404 Not Found` if sandbox doesn't exist.

---

### Upload File

```
PUT /api/v1/hiveboxes/:id/files?path=/path/in/sandbox
```

Uploads a file into the sandbox filesystem.

**Query parameters**:
- `path`: absolute path where the file should be created inside the sandbox

**Request body**: raw file content (binary)

**Response**: `200 OK`

---

### Download File

```
GET /api/v1/hiveboxes/:id/files?path=/path/in/sandbox
```

Downloads a file from the sandbox filesystem.

**Query parameters**:
- `path`: absolute path to the file inside the sandbox

**Response** (`200 OK`): raw file content

**Errors**: `404 Not Found` if file doesn't exist.

---

### List Files

```
GET /api/v1/hiveboxes/:id/files/list?path=/path/in/sandbox
```

Lists all files and directories recursively from the given path inside the sandbox.

**Query parameters**:
- `path` (optional): directory path inside the sandbox (default: `/`)

**Response** (`200 OK`):
```json
{
  "path": "/workspace",
  "entries": [
    { "name": "src", "path": "/workspace/src", "entry_type": "directory" },
    { "name": "main.rs", "path": "/workspace/src/main.rs", "entry_type": "file", "size": 1234 },
    { "name": "README.md", "path": "/workspace/README.md", "entry_type": "file", "size": 567 }
  ],
  "total": 3
}
```

Each entry includes:
| Field | Description |
|-------|-------------|
| `name` | File or directory name |
| `path` | Full logical path inside the sandbox |
| `entry_type` | `"file"` or `"directory"` |
| `size` | Size in bytes (files only, omitted for directories) |

**Errors**: `404 Not Found` if sandbox doesn't exist. `500` if path is not a directory.

---

### Destroy Sandbox

```
DELETE /api/v1/hiveboxes/:id
```

Destroys a sandbox and cleans up all resources.

**Response** (`200 OK`):
```json
{
  "id": "my-sandbox",
  "status": "destroyed"
}
```

**Errors**: `404 Not Found` if sandbox doesn't exist.

---

### Analytics

```
GET /api/v1/analytics
```

Returns metrics history for all sandboxes and host resource usage.

**Query parameters**:
- `range` (optional): time range in seconds (e.g., `300` = 5 min, `3600` = 1 hour). Default: all available history.

**Response** (`200 OK`):
```json
{
  "samples": [
    {
      "timestamp": 1705312200,
      "sandbox_count": 2,
      "sandboxes": [],
      "host_memory_total": 8388608000,
      "host_memory_used": 4194304000,
      "host_cpu_percent": 12.5
    }
  ],
  "interval_secs": 10
}
```

---

## MCP Bridge

HiveBox exposes sandbox operations as MCP (Model Context Protocol) tools, allowing any MCP-compatible AI client (OpenCode, Claude Code, etc.) to operate inside sandboxes.

### MCP over HTTP (recommended)

```
POST /api/v1/hiveboxes/:id/mcp
```

The daemon serves MCP directly over HTTP. No local binary needed — just a URL.

Accepts JSON-RPC 2.0 requests in the body, returns JSON-RPC responses. This is the **Streamable HTTP** MCP transport.

Configure in your AI client:
```json
{
  "mcpServers": {
    "sandbox": {
      "url": "http://your-server:7070/api/v1/hiveboxes/my-sandbox/mcp",
      "headers": { "Authorization": "Bearer your-secret-key" }
    }
  }
}
```

### MCP over stdio (local)

For local setups, you can also use the `hivebox mcp` CLI command which communicates over stdin/stdout:

```json
{
  "mcpServers": {
    "sandbox": {
      "command": "hivebox",
      "args": ["mcp", "--sandbox", "my-sandbox", "--api-url", "http://localhost:7070"],
      "env": { "HIVEBOX_API_KEY": "your-secret-key" }
    }
  }
}
```

### Available tools

Both transports expose 14 tools:

| Tool | Description |
|------|-------------|
| `exec` | Execute a shell command |
| `read_file` | Read file contents (with optional head/tail) |
| `read_multiple_files` | Read multiple files at once |
| `write_file` | Create or overwrite a file |
| `edit_file` | Targeted text replacement |
| `list_directory` | List directory contents |
| `directory_tree` | Recursive tree view |
| `search_files` | Search for text patterns (grep) |
| `glob` | Find files matching glob patterns (e.g. `**/*.ts`, `*.json`) |
| `get_file_info` | File metadata (stat) |
| `create_directory` | Create directories |
| `move_file` | Move or rename files |
| `read_media_file` | Read images/media as base64 with MIME type |
| `list_directory_with_sizes` | List directory with human-readable sizes |

File upload/download is available via the REST API endpoints (`PUT/GET /api/v1/hiveboxes/:id/files`).

---

## OpenCode Agent (per-sandbox AI)

Each sandbox automatically gets its own [OpenCode](https://opencode.ai) serve instance. The daemon proxies all requests through a single port so you can interact with the AI agent for any sandbox without managing separate ports.

### Configuration

OpenCode is enabled by default. To disable it, set the `HIVEBOX_OPENCODE` environment variable:

```bash
# OpenCode enabled (default)
docker run --privileged --cgroupns=host -p 7070:7070 hivebox

# OpenCode disabled
docker run --privileged --cgroupns=host -p 7070:7070 -e HIVEBOX_OPENCODE=false hivebox
```

| Value | Behavior |
|-------|----------|
| *(unset)* | Enabled (default) |
| `true`, `1`, `yes` | Enabled |
| `false`, `0` | Disabled — no opencode serve is spawned, agent endpoints return 404 |

### Global environment variables

These env vars set defaults for all sandboxes. Per-sandbox overrides can be passed at creation time via the API.

```bash
docker run --privileged --cgroupns=host -p 7070:7070 \
  -e HIVEBOX_API_KEY=secret \
  -e HIVEBOX_OPENCODE_BASE_URL=https://api.openai.com/v1 \
  -e HIVEBOX_OPENCODE_API_KEY=sk-... \
  -e HIVEBOX_OPENCODE_MODEL=gpt-4o \
  -e HIVEBOX_OPENCODE_SKILLS_PATH=/skills \
  -e 'HIVEBOX_OPENCODE_MCPS={"my-server":{"type":"remote","url":"https://example.com/mcp","enabled":true}}' \
  -v /host/path/to/skills:/skills \
  hivebox
```

| Variable | Description |
|----------|-------------|
| `HIVEBOX_OPENCODE_BASE_URL` | Default LLM base URL for all sandboxes |
| `HIVEBOX_OPENCODE_API_KEY` | Default LLM API key for all sandboxes |
| `HIVEBOX_OPENCODE_MODEL` | Default LLM model for all sandboxes |
| `HIVEBOX_OPENCODE_SKILLS_PATH` | Custom skills folder to use instead of the built-in Anthropic skills. Mount it into the container and point here (default: `/root/.config/opencode/skills`) |
| `HIVEBOX_OPENCODE_MCPS` | JSON object of global MCP servers added to every sandbox |
| `HIVEBOX_OPENCODE_INSTRUCTIONS` | Custom agent instructions (newline-separated) |

### How it works

1. When a sandbox is created, the daemon spawns `opencode serve` on an internal port (14000+)
2. OpenCode is preconfigured with an MCP server pointing to the sandbox's MCP endpoint
3. All requests are proxied through the daemon on the same port (7070)

The `opencode_url` field in the create/list response indicates the proxy base path.

### Base path

```
/api/v1/hiveboxes/:id/opencode/
```

All [OpenCode Server API](https://opencode.ai/docs/server) endpoints are available under this path. The daemon forwards method, headers, and body transparently.

### Quick start

**1. Create a sandbox** (opencode starts automatically):

```bash
curl -X POST http://localhost:7070/api/v1/hiveboxes \
  -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" \
  -d '{"image":"base"}'
# Response includes: "opencode_url": "/api/v1/hiveboxes/hb-xxx/opencode/"
```

**2. Check health**:

```bash
curl http://localhost:7070/api/v1/hiveboxes/hb-xxx/opencode/global/health \
  -H "Authorization: Bearer $KEY"
# {"healthy":true,"version":"1.2.24"}
```

**3. Connect to the SSE event stream** (real-time events):

```bash
curl -N http://localhost:7070/api/v1/hiveboxes/hb-xxx/opencode/event \
  -H "Authorization: Bearer $KEY"
# data: {"type":"server.connected","properties":{}}
# data: {"type":"session.status",...}
# data: {"type":"message.part.delta",...}  (token-by-token streaming)
```

**4. Create a session**:

```bash
curl -X POST http://localhost:7070/api/v1/hiveboxes/hb-xxx/opencode/session \
  -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" \
  -d '{}'
# {"id":"ses_xxx","slug":"brave-forest",...}
```

**5. Send a prompt** (async — response streams via SSE):

```bash
curl -X POST http://localhost:7070/api/v1/hiveboxes/hb-xxx/opencode/session/ses_xxx/prompt_async \
  -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" \
  -d '{"parts":[{"type":"text","text":"create a hello world python script"}]}'
```

**6. Abort a running response**:

```bash
curl -X POST http://localhost:7070/api/v1/hiveboxes/hb-xxx/opencode/session/ses_xxx/abort \
  -H "Authorization: Bearer $KEY"
```

### Key SSE event types

| Event | Description |
|-------|-------------|
| `server.connected` | SSE connection established |
| `session.status` | Status change (`busy` / `idle`) |
| `message.part.delta` | Token-by-token text streaming |
| `message.part.updated` | Part completed (step-start, step-finish, tool-invocation, tool-result) |
| `session.updated` | Session metadata changed (title, summary) |
| `session.idle` | Agent finished processing |

### Available proxy endpoints

All OpenCode server endpoints are proxied. Key ones:

| Method | Path | Description |
|--------|------|-------------|
| GET | `/global/health` | Health check |
| GET | `/event` | SSE event stream |
| GET | `/session` | List sessions |
| POST | `/session` | Create session |
| GET | `/session/:id` | Get session details |
| DELETE | `/session/:id` | Delete session |
| POST | `/session/:id/message` | Send message (sync) |
| POST | `/session/:id/prompt_async` | Send message (async, use with SSE) |
| POST | `/session/:id/abort` | Abort running response |
| GET | `/session/:id/message` | List messages |
| GET | `/provider` | List available AI providers/models |
| GET | `/config` | Get configuration |
| GET | `/doc` | OpenAPI 3.1 spec |

### Dashboard

The web dashboard at `/dashboard` includes an **Agent** tab in the sidebar for each sandbox. Click the blue "Agent" button on any sandbox row to open a real-time chat interface that connects via SSE and streams events directly.

---

## Error Responses

All errors return JSON:

```json
{
  "error": "sandbox 'xyz' not found"
}
```

Common HTTP status codes:
| Code | Meaning |
|------|---------|
| 200 | Success |
| 201 | Created |
| 400 | Bad request (invalid input) |
| 401 | Unauthorized (missing/wrong API key) |
| 404 | Not found (sandbox/file doesn't exist) |
| 500 | Internal server error |
