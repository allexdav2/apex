<!-- status: FUTURE -->
# MCP Distribution & Marketplace Publishing Plan

> **For agentic workers:** Execute steps sequentially. Each step has a verification command. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Get APEX listed on all major MCP marketplaces and AI tool directories so users can discover and install it from their preferred tool.

**Architecture:** Publish npm/pypi packages first (prerequisites), then register on the official MCP Registry (cascades to aggregators), then submit to individual marketplaces.

**Tech Stack:** mcp-publisher CLI, npm, twine, GitHub Releases

---

## Prerequisites

These must be done before any marketplace submission:

### Task 0: Tag release and publish packages

- [ ] **Step 1: Verify `apex mcp` works**

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}' | apex mcp 2>/dev/null | head -1
```
Expected: JSON response with `serverInfo`

- [ ] **Step 2: Update versions**

Sync version `0.1.0` across:
- `Cargo.toml` (workspace)
- `npm/package.json`
- `python/pyproject.toml`
- `python/apex_cli/__init__.py`
- `HomebrewFormula/apex.rb`

- [ ] **Step 3: Tag and push**

```bash
git tag v0.1.0
git push --tags
```
CI builds binaries for 4 targets (linux-x64, linux-arm64, macos-x64, macos-arm64).

- [ ] **Step 4: Publish npm**

```bash
cd npm/
npm publish --access public
```
Verify: `npx @apex-coverage/cli mcp` starts the server.

- [ ] **Step 5: Publish PyPI**

```bash
cd python/
python -m build
twine upload dist/*
```
Verify: `pipx run apex-coverage mcp` starts the server.

- [ ] **Step 6: Update Homebrew formula SHA**

Update `HomebrewFormula/apex.rb` with SHA256 from the GitHub Release assets.

---

## Task 1: Official MCP Registry

**Priority: Highest** — consumed by all MCP clients. Other aggregators index from here.

**File:** `integrations/mcp-registries/server.json`

- [ ] **Step 1: Install mcp-publisher**

```bash
brew install mcp-publisher
# or: cargo install mcp-publisher
```

- [ ] **Step 2: Authenticate**

```bash
mcp-publisher login github
```

- [ ] **Step 3: Validate**

```bash
mcp-publisher publish --dry-run --server integrations/mcp-registries/server.json
```
Expected: Validation passes, no errors.

- [ ] **Step 4: Publish**

```bash
mcp-publisher publish --server integrations/mcp-registries/server.json
```

- [ ] **Step 5: Verify**

Check https://registry.modelcontextprotocol.io — search for "apex".

---

## Task 2: npm discoverability

**File:** `npm/package.json`

- [ ] **Step 1: Add MCP keywords to package.json**

```json
{
  "keywords": [
    "mcp",
    "mcp-server",
    "model-context-protocol",
    "modelcontextprotocol",
    "coverage",
    "security",
    "sast",
    "code-analysis",
    "ai"
  ]
}
```

- [ ] **Step 2: Verify bin field**

```json
{
  "bin": {
    "apex": "./bin/apex"
  }
}
```

- [ ] **Step 3: Republish**

```bash
cd npm/ && npm publish --access public
```

- [ ] **Step 4: Verify**

```bash
npm search mcp-server coverage
```
APEX should appear in results.

---

## Task 3: Smithery.ai

**File:** `integrations/smithery.yaml`

- [ ] **Step 1: Copy smithery.yaml to repo root**

```bash
cp integrations/smithery.yaml smithery.yaml
git add smithery.yaml && git commit -m "chore: add smithery.yaml for MCP marketplace"
git push
```

- [ ] **Step 2: Submit**

1. Go to https://smithery.ai/new
2. Sign in with GitHub
3. Select `allexdav2/apex` repository
4. Smithery auto-reads `smithery.yaml`
5. Review and submit

- [ ] **Step 3: Verify**

Check https://smithery.ai — search for "apex".

---

## Task 4: Glama.ai

**Effort: Zero** — auto-indexes from GitHub.

- [ ] **Step 1: Ensure README mentions MCP**

Add to README.md:

```markdown
## MCP Server

APEX exposes its analysis tools via the Model Context Protocol (MCP):

\`\`\`bash
apex mcp
\`\`\`

Works with Cursor, Codex CLI, Cline, Continue.dev, LM Studio, and any MCP-compatible client.
```

- [ ] **Step 2: Verify**

Check https://glama.ai/mcp/servers — search for "apex" (may take a few days to index).

---

## Task 5: Cline MCP Marketplace

- [ ] **Step 1: Create 400x400 PNG logo**

Simple logo for APEX — coverage bar graph or shield icon.

- [ ] **Step 2: Open issue**

Go to https://github.com/cline/mcp-marketplace and open an issue with:
- Title: "Add APEX — Code Coverage & Security Analysis MCP Server"
- Body:
  - GitHub URL: https://github.com/allexdav2/apex
  - Logo: attach 400x400 PNG
  - Description: "Coverage analysis and security detection for 11 languages. 6 MCP tools: coverage gaps, CWE-mapped findings, reverse path reachability, CI gate, prerequisites check, deployment scoring."
  - Install: `npm install -g @apex-coverage/cli`
  - Usage: Add to MCP config: `{ "command": "apex", "args": ["mcp"] }`

- [ ] **Step 3: Wait for review (~2 days)**

---

## Task 6: Cursor Directory

- [ ] **Step 1: Submit**

Go to https://cursor.directory/plugins and submit:
- Name: APEX
- Description: Code coverage & security analysis for 11 languages
- MCP config: `{ "command": "apex", "args": ["mcp"] }`
- GitHub: https://github.com/allexdav2/apex
- Install: `brew install allexdav2/tap/apex` or `npm i -g @apex-coverage/cli`

---

## Task 7: Per-tool config templates in README

- [ ] **Step 1: Add "AI Tool Integration" section to README**

```markdown
## AI Tool Integration

APEX works with any MCP-compatible AI coding assistant.

### Cursor
Copy to `.cursor/mcp.json`:
\`\`\`json
{ "mcpServers": { "apex": { "command": "apex", "args": ["mcp"] } } }
\`\`\`

### Codex CLI
Add to `.codex/config.toml`:
\`\`\`toml
[mcp_servers.apex]
type = "stdio"
command = "apex"
args = ["mcp"]
\`\`\`

### Cline
In Cline MCP settings, add:
\`\`\`json
{ "apex": { "command": "apex", "args": ["mcp"] } }
\`\`\`

### Continue.dev
Save to `.continue/mcpServers/apex.json`:
\`\`\`json
{ "apex": { "command": "apex", "args": ["mcp"] } }
\`\`\`
```

- [ ] **Step 2: Commit and push**

---

## Execution Order

```
Task 0: Tag release, publish npm + pypi (prerequisite for all)
  ↓
Task 1: Official MCP Registry (unlocks aggregators)
  ↓ (parallel after Task 1)
Task 2: npm keywords
Task 3: Smithery.ai
Task 4: Glama.ai (automatic)
Task 5: Cline Marketplace
Task 6: Cursor Directory
Task 7: README update
```

## Expected Result

After completing all tasks, APEX will be discoverable from:

| Where | How users find it |
|-------|------------------|
| Official MCP Registry | Search "coverage" or "security" |
| Smithery.ai | Browse or search |
| Glama.ai | Auto-indexed |
| Cline Marketplace | Browse MCP servers |
| Cursor Directory | Browse plugins |
| npm | `npm search mcp coverage` |
| PyPI | `pip search apex-coverage` |
| Homebrew | `brew search apex` |

**One `apex mcp` command, 8 discovery channels.**
