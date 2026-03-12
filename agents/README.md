# APEX Agents for Claude Code

Claude Code agents and slash commands for working with APEX.

## Install

```bash
./agents/install.sh
```

This copies agents and commands into `.claude/` so Claude Code picks them up immediately.

## Environment

Set `APEX_HOME` to point to the APEX repo checkout:

```bash
export APEX_HOME=/path/to/apex
```

If not set, commands assume APEX is in the current working directory's git root.

For Rust coverage, also set:

```bash
export LLVM_COV=/opt/homebrew/opt/llvm/bin/llvm-cov
export LLVM_PROFDATA=/opt/homebrew/opt/llvm/bin/llvm-profdata
```

## Agents (auto-invoked by Claude)

| Agent | Trigger |
|-------|---------|
| `apex-coverage-analyst` | "what's our coverage?", "which parts are uncovered?" |
| `apex-test-writer` | "write tests for X", "improve coverage in Y" |
| `apex-runner` | "run apex against Z", "run apex on itself" |
| `apex-sdlc-analyst` | "what's our deploy score?", "find flaky tests", "show hot paths" |

## Slash Commands (user-invoked)

| Command | Usage |
|---------|-------|
| `/apex` | **Main entrypoint** — dashboard with deploy score, key findings, recommendations |
| `/apex-run [target] [lang]` | Full autonomous coverage loop |
| `/apex-index [target] [lang]` | Build per-test branch index for intelligence commands |
| `/apex-intel [target]` | Full SDLC intelligence report |
| `/apex-deploy [target] [lang]` | Deployment readiness check |
| `/apex-status [crate]` | Show coverage table |
| `/apex-gaps [crate-or-file]` | List uncovered regions with explanations |
| `/apex-generate <crate-or-file>` | Generate tests for uncovered code |
| `/apex-ci [min-coverage]` | Check CI coverage gate |
