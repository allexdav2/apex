<!-- status: DONE -->

# GitHub Org Rename: allexdav2 → sahajamoth

> **For agentic workers:** Mechanical find-replace across 23 files. All tasks independent — dispatch in parallel.

**Goal:** Update all references from `allexdav2` to `sahajamoth` across the entire codebase — GitHub URLs, Homebrew tap, MCP registry IDs, author fields, installation commands, docs.

**Architecture:** Pure string replacement. No logic changes. 5 pattern types:

| Pattern | Old | New |
|---------|-----|-----|
| GitHub URL | `github.com/allexdav2/apex` | `github.com/sahajamoth/apex` |
| Homebrew tap | `allexdav2/tap/apex` | `sahajamoth/tap/apex` |
| Author/org | `allexdav2` | `sahajamoth` |
| MCP registry ID | `io.github.allexdav2/apex` | `io.github.sahajamoth/apex` |
| Nix flake | `github:allexdav2/apex` | `github:sahajamoth/apex` |

**Note:** Email `allexdav@gmail.com` is NOT renamed (it's an email address, not a GitHub handle).

---

## Task 1: Core project files (5 files)

**Files:**
- `Cargo.toml` — repository URL
- `CLAUDE.md` — install commands
- `README.md` — badges, install commands, clone URL
- `CHANGELOG.md` — install commands
- `CONTRIBUTING.md` — clone URL

- [ ] Replace all `allexdav2` with `sahajamoth` (except email addresses)
- [ ] Verify: `grep -r allexdav2 Cargo.toml CLAUDE.md README.md CHANGELOG.md CONTRIBUTING.md`

---

## Task 2: Distribution files (6 files)

**Files:**
- `install.sh` — REPO variable, curl URL
- `HomebrewFormula/apex.rb` — homepage, download URLs
- `npm/package.json` — repository URL
- `npm/install.js` — REPO constant
- `python/pyproject.toml` — repository URLs
- `python/apex_cli/__init__.py` — REPO constant
- `flake.nix` — homepage URL

- [ ] Replace all `allexdav2` with `sahajamoth`
- [ ] Verify: `grep -r allexdav2 install.sh HomebrewFormula/ npm/ python/ flake.nix`

---

## Task 3: Integration configs (7 files)

**Files:**
- `integrations/mcp-registries/server.json`
- `integrations/mcp-registries/mcp-registry.json`
- `integrations/mcp-registries/mcp-run.json`
- `integrations/mcp-registries/smithery.yaml`
- `integrations/a2a/agent-card.json`
- `integrations/oap/@apex/coverage-runner/manifest.toml`
- `integrations/oap/@apex/security-analyst/manifest.toml`
- `integrations/oap/@apex/mcp-server/manifest.toml`

- [ ] Replace all `allexdav2` with `sahajamoth`
- [ ] Verify: `grep -r allexdav2 integrations/`

---

## Task 4: Documentation (2 files)

**Files:**
- `docs/superpowers/plans/2026-03-15-mcp-distribution.md`
- `docs/superpowers/plans/2026-03-15-agent-distribution-protocols.md`
- `PUBLISHING.md`

- [ ] Replace all `allexdav2` with `sahajamoth`
- [ ] Verify: `grep -r allexdav2 docs/ PUBLISHING.md`

---

## Verification

```bash
# Must return zero results (except possibly git history)
grep -r allexdav2 --include='*.md' --include='*.toml' --include='*.json' --include='*.yaml' --include='*.yml' --include='*.rb' --include='*.sh' --include='*.js' --include='*.py' --include='*.nix' .
```
