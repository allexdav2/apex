<!-- status: ACTIVE -->
# APEX MCP Server Design

## Overview

Add `apex mcp` subcommand that runs an MCP (Model Context Protocol) STDIO server, making APEX's analysis tools available to any MCP-compatible AI coding assistant. Plus `apex integrate` to auto-configure detected tools.

## Motivation

APEX currently works only via CLI or Claude Code agents. An MCP server makes APEX a universal tool for Cursor, Codex CLI, Cline, Continue.dev, LM Studio, and any future MCP client — one integration covers the entire ecosystem.

## Architecture

```
apex mcp
  ├── STDIO transport (stdin/stdout JSON-RPC)
  ├── 6 tool definitions
  ├── Calls existing apex-cli handlers directly (no subprocess)
  └── Returns JSON results

apex integrate [tool]
  ├── Auto-detects installed tools
  ├── Writes per-tool MCP config files
  └── Verifies apex binary is in PATH
```

## MCP Server (`apex mcp`)

### Transport

STDIO — reads JSON-RPC from stdin, writes responses to stdout. This is the universal MCP transport supported by all clients.

### Dependencies

Add `rmcp` crate (Rust MCP SDK) to `apex-cli/Cargo.toml`:
```toml
rmcp = { version = "0.1", features = ["server", "transport-io"] }
```

If `rmcp` is too heavy or immature, fall back to raw JSON-RPC over stdin/stdout using `serde_json` + `tokio::io` (the protocol is simple — `initialize`, `tools/list`, `tools/call`).

### Tools Exposed

#### 1. `apex_run` — Coverage gap analysis
```json
{
  "name": "apex_run",
  "description": "Run coverage analysis on a project. Returns uncovered branches, gap report, and coverage percentage.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "target": { "type": "string", "description": "Path to project root" },
      "lang": { "type": "string", "enum": ["python", "rust", "javascript", "java", "go", "ruby", "cpp", "swift", "csharp", "kotlin"] },
      "strategy": { "type": "string", "enum": ["agent", "fuzz", "concolic", "all"], "default": "agent" },
      "output_format": { "type": "string", "enum": ["json", "text"], "default": "json" }
    },
    "required": ["target", "lang"]
  }
}
```

#### 2. `apex_detect` — Security analysis
```json
{
  "name": "apex_detect",
  "description": "Run security detectors on a project. Returns findings with CWE IDs, severity, and remediation advice.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "target": { "type": "string" },
      "lang": { "type": "string" },
      "severity_threshold": { "type": "string", "enum": ["low", "medium", "high", "critical"], "default": "low" }
    },
    "required": ["target", "lang"]
  }
}
```

#### 3. `apex_reach` — Reverse path analysis
```json
{
  "name": "apex_reach",
  "description": "Find all entry points (tests, HTTP handlers, main) that can reach a specific code location.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "target": { "type": "string", "description": "file:line format" },
      "lang": { "type": "string" },
      "granularity": { "type": "string", "enum": ["function", "block", "line"], "default": "function" },
      "entry_kind": { "type": "string", "enum": ["test", "http", "main", "api", "cli"] }
    },
    "required": ["target", "lang"]
  }
}
```

#### 4. `apex_ratchet` — CI coverage gate
```json
{
  "name": "apex_ratchet",
  "description": "Check if project coverage meets minimum threshold. Returns PASS/FAIL.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "target": { "type": "string" },
      "lang": { "type": "string" },
      "min_coverage": { "type": "number", "description": "Minimum coverage ratio 0.0-1.0", "default": 0.8 }
    },
    "required": ["target", "lang"]
  }
}
```

#### 5. `apex_doctor` — Prerequisites check
```json
{
  "name": "apex_doctor",
  "description": "Check that all required tools are installed for a given language.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "lang": { "type": "string" }
    }
  }
}
```

#### 6. `apex_deploy_score` — Deployment readiness
```json
{
  "name": "apex_deploy_score",
  "description": "Calculate deployment confidence score (0-100) based on coverage, security findings, and test health.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "target": { "type": "string" },
      "lang": { "type": "string" }
    },
    "required": ["target", "lang"]
  }
}
```

### Implementation Pattern

Each tool call invokes the existing handler function directly — no subprocess spawn:

```rust
async fn handle_tool_call(name: &str, args: Value) -> Result<Value> {
    match name {
        "apex_run" => {
            let target = args["target"].as_str().unwrap_or(".");
            let lang = args["lang"].as_str().unwrap_or("python");
            // Construct RunArgs, call run() directly
            let result = run(run_args, &cfg).await?;
            Ok(serde_json::to_value(result)?)
        }
        // ... other tools
    }
}
```

### Server Lifecycle

1. Client connects via STDIO
2. Client sends `initialize` → server responds with capabilities (tools)
3. Client sends `tools/list` → server returns 6 tool definitions
4. Client sends `tools/call` with tool name + arguments → server executes, returns result
5. Repeat until client disconnects

### Logging

All `tracing` output goes to stderr (not stdout — stdout is the MCP protocol channel). This is already the case since apex-cli initializes tracing with `stderr` writer.

## Integration Command (`apex integrate`)

### Subcommand

```
apex integrate [--tool <name>] [--global]
```

- No args: auto-detect all installed tools, configure all
- `--tool cursor|codex|cline|continue|lm-studio`: configure specific tool
- `--global`: write to global config (default: per-project)

### Auto-Detection

| Tool | Detection | Config Written |
|------|-----------|---------------|
| Cursor | `.cursor/` dir or `cursor` in PATH | `.cursor/mcp.json` |
| Codex CLI | `codex` in PATH or `.codex/` dir | `.codex/config.toml` |
| Cline | VS Code extension dir check | Print instructions (Cline config is in VS Code settings) |
| Continue | `.continue/` dir | `.continue/mcpServers/apex.json` |
| LM Studio | `lm-studio` in PATH | Print instructions (global config) |

### Config Templates

**Cursor** (`.cursor/mcp.json`):
```json
{
  "mcpServers": {
    "apex": {
      "command": "apex",
      "args": ["mcp"],
      "env": {}
    }
  }
}
```

**Codex CLI** (`.codex/config.toml`):
```toml
[mcp_servers.apex]
type = "stdio"
command = "apex"
args = ["mcp"]
```

**Continue.dev** (`.continue/mcpServers/apex.json`):
```json
{
  "apex": {
    "command": "apex",
    "args": ["mcp"]
  }
}
```

### Output

```
$ apex integrate
Detected: Cursor, Codex CLI, Continue.dev

  ✓ .cursor/mcp.json — written
  ✓ .codex/config.toml — updated (added [mcp_servers.apex])
  ✓ .continue/mcpServers/apex.json — written

APEX MCP server configured for 3 tools.
Run "apex mcp" to start the server, or tools will start it automatically.
```

## Files

| File | What |
|------|------|
| `crates/apex-cli/src/mcp.rs` | MCP server implementation |
| `crates/apex-cli/src/integrate.rs` | `apex integrate` command |
| `crates/apex-cli/src/lib.rs` | Add `Mcp` and `Integrate` to Commands enum |
| `crates/apex-cli/Cargo.toml` | Add `rmcp` or raw JSON-RPC deps |

## Testing

- Unit tests for tool definition generation
- Unit tests for config template output (verify JSON/TOML is valid)
- Integration test: spawn `apex mcp` as subprocess, send `initialize` + `tools/list` over stdin, verify response
- Integration test: send `tools/call` for `apex_doctor`, verify structured response

## Fallback: Raw JSON-RPC

If `rmcp` is unavailable or too heavy, implement MCP manually:

```rust
// Read line from stdin
let line = stdin.read_line().await?;
let request: JsonRpcRequest = serde_json::from_str(&line)?;

match request.method.as_str() {
    "initialize" => respond_with_capabilities(),
    "tools/list" => respond_with_tool_definitions(),
    "tools/call" => {
        let result = handle_tool_call(&request.params).await?;
        respond_with_result(result)
    }
    _ => respond_with_error("method not found"),
}
```

The MCP STDIO protocol is ~100 lines of JSON-RPC handling. No framework needed.
