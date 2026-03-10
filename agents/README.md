# APEX Agent Marketplace

Claude Code agents and slash commands for working with APEX.

## Install

```bash
./agents/install.sh
```

This copies agents and commands into `.claude/` so Claude Code picks them up immediately.

## Agents (auto-invoked by Claude)

| Agent | Trigger |
|-------|---------|
| `apex-coverage-analyst` | "what's our coverage?", "which parts are uncovered?" |
| `apex-test-writer` | "write tests for X", "improve coverage in Y" |
| `apex-runner` | "run apex against Z", "run apex on itself" |

## Slash Commands (user-invoked)

| Command | Usage |
|---------|-------|
| `/apex-run [target] [lang] [strategy]` | Run APEX against a target |
| `/apex-status [crate]` | Show coverage table |
| `/apex-gaps [crate-or-file]` | List uncovered regions with explanations |
| `/apex-generate <crate-or-file>` | Generate tests for uncovered code |
| `/apex-ci [min-coverage]` | Check CI coverage gate |

## Prerequisites

```bash
cargo install cargo-llvm-cov
# On Homebrew Rust, also set:
export LLVM_COV=/opt/homebrew/opt/llvm/bin/llvm-cov
export LLVM_PROFDATA=/opt/homebrew/opt/llvm/bin/llvm-profdata
```
