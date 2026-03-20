# APEX Development Plan

*Created: March 15, 2026*

## Context

Research session benchmarked 8 local models via Ollama. Key findings: qwen3-coder:30b at 103 t/s is fastest, nemotron-3-super has best agentic reasoning, qwen3-coder-next is purpose-built for coding agents. Ollama's Anthropic-compatible API means APEX can switch between local and API models with a base URL change. Codex CLI and OpenClaw provide free alternative execution environments.

---

## Phase 1: Local Model Integration (This Week)

### 1.1 Benchmark Models on APEX Tasks
- Run each local model on actual APEX synthesis tasks (not generic coding prompts):
  - **Test generation**: given uncovered branch + context, generate a test
  - **Security pattern detection**: given code snippet, identify vulnerabilities
  - **Coverage analysis**: given test output, extract branch coverage data
- Models to benchmark:
  | Model | TPS | Why |
  |-------|-----|-----|
  | qwen3-coder:30b | 103 | Fastest, good coding |
  | qwen3-coder-next | 39 | Best coding quality |
  | nemotron-3-super | 31 | Best agentic reasoning |
  | glm-4.7-flash | 67 | Balanced speed/quality |
  | gpt-oss:120b | 66 | Strong general, MIT |
- Measure: test quality (compilable? covers target branch?), false positive rate, tokens consumed

### 1.2 Ollama Backend for apex-synth
- Add configurable model provider to apex-synth crate:
  ```toml
  [synthesis]
  provider = "ollama"              # ollama | anthropic | openai
  base_url = "http://localhost:11434"
  model = "qwen3-coder:30b"
  fallback_model = "qwen3-coder-next"  # for retries on failure
  ```
- Use Ollama's Anthropic-compatible API (`/v1/messages`) — minimal code change
- Keep existing Anthropic API as default, Ollama as opt-in

### 1.3 Cost Analysis: Local vs API
- Run same synthesis workload on:
  - API (Claude Sonnet): measure cost in USD
  - Local (qwen3-coder:30b): measure time + electricity
- Quantify: at what volume does local become cheaper?
- Consider: GPU utilization during APEX's non-synthesis phases (wasted compute)

---

## Phase 2: Multi-Model Strategy (2 Weeks)

### 2.1 Tiered Model Pipeline
- **Fast model** (qwen3-coder:30b, 103 t/s) → initial test generation, high throughput
- **Quality model** (qwen3-coder-next or nemotron) → refine failed tests, complex synthesis
- **Reasoning model** (nemotron-3-super or API) → security analysis, architectural decisions
- Pipeline:
  ```
  uncovered branch → fast model generates test →
    compile? → yes → done
            → no  → quality model refines →
                     compile? → yes → done
                             → no  → reasoning model (or API fallback)
  ```
- Expected: 70-80% resolved by fast model, 15-20% by quality, <5% needs API

### 2.2 Ollama Backend for apex-agent
- Extend apex-agent crate with same provider abstraction as apex-synth
- apex-agent handles orchestration, priority scheduling, multi-target planning
- Needs: stronger reasoning capability → default to nemotron-3-super or API
- Local model viability depends on Phase 1 benchmark results

### 2.3 Model Selection Heuristic
- Implement automatic model selection based on task complexity:
  - Simple test (single branch, straightforward logic) → fast model
  - Complex test (mocking, async, multi-step setup) → quality model
  - Security/architectural analysis → reasoning model
- Heuristic inputs: branch complexity score, dependency depth, language

---

## Phase 3: External Integration (1 Month)

### 3.1 OpenClaw Notifications
- APEX findings → Slack/Discord via OpenClaw gateway
- Use case: APEX runs in CI, security findings pushed to team chat
- Implementation:
  - apex-rpc emits findings as structured events
  - OpenClaw gateway subscribes and routes to configured channels
- Ref: OpenClaw installed at /opt/homebrew/bin/openclaw

### 3.2 Codex CLI Integration
- Run APEX's AI synthesis via `codex --oss` for zero-cost agentic test writing
- Codex has built-in file read/write + shell execution — similar to Claude Code
- Evaluate: can Codex replace Claude Code for APEX's agent workflows?

### 3.3 APEX Agents via Agent SDK
- Extract apex-agent orchestration as standalone Agent SDK application
- Use case: APEX coverage daemon running as a service (not inside Claude Code)
- Agent SDK provides: tool execution, context management, subagent spawning
- Target: `apex-daemon` binary that runs continuously, watches for code changes, auto-generates tests

### 3.4 Fleet Deep Integration
- Fleet crews auto-trigger APEX scans on code changes in their owned paths
- Implementation:
  - Fleet crew's PostToolUse hook detects file writes
  - Triggers APEX scan on changed files
  - APEX findings injected into crew's review context
- Requires: APEX CLI callable from Fleet hook scripts

---

## Phase 4: Scale & Optimization (2 Months)

### 4.1 Distributed Synthesis
- Use apex-rpc to distribute synthesis across multiple Ollama instances
- One machine runs fast model, another runs quality model
- Load balance based on queue depth and model availability

### 4.2 Fine-Tuning Evaluation
- Evaluate: fine-tune qwen3-coder:30b on APEX's test generation task
- Training data: successful test generations from Phase 1-2 (input: branch context, output: passing test)
- Target: specialized model that outperforms general-purpose on APEX-specific synthesis

### 4.3 Coverage-Guided Model Selection
- Feed coverage feedback into model selection:
  - Track which model successfully covers which branch patterns
  - Build a classifier: (branch features) → (best model)
  - Over time, system learns which model works best for which code patterns

---

## Local Model Quick Reference

| Alias | Model | TPS | Size | Best For |
|-------|-------|-----|------|----------|
| `qclaude` | qwen3-coder:30b | 103 | 18 GB | Fast synthesis, iteration |
| `qcclaude` | qwen3-coder-next | 39 | 52 GB | Complex test generation |
| `nclaude` | nemotron-3-super | 31 | 87 GB | Agent orchestration, reasoning |
| `gfclaude` | glm-4.7-flash | 67 | 19 GB | Balanced workloads |
| `oclaude` | gpt-oss:120b | 66 | 65 GB | General purpose, MIT licensed |

## Success Metrics

| Phase | Metric | Target |
|-------|--------|--------|
| 1 | Local model test quality | ≥60% of API quality |
| 1 | Cost reduction | Quantified $/1000 tests local vs API |
| 2 | Fast model resolution rate | ≥70% of branches |
| 2 | Overall synthesis success | ≥90% (across all tiers) |
| 3 | CI notification latency | Findings in Slack within 5 min |
| 3 | Standalone daemon | Running without Claude Code |
| 4 | Fine-tuned model improvement | ≥15% over base model |
