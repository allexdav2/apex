# APEX MCP Server

This agent provides code coverage analysis and security detection via MCP tools.

## Available Tools

- **apex_run** — Coverage gap analysis. Pass `target` (project path) and `lang`.
- **apex_detect** — Security findings with CWE IDs. Pass `target` and `lang`.
- **apex_reach** — Find entry points reaching a file:line. Pass `target` (file:line format) and `lang`.
- **apex_ratchet** — CI coverage gate. Returns PASS/FAIL.
- **apex_doctor** — Check prerequisites.
- **apex_deploy_score** — Deployment confidence 0-100.

## Supported Languages

python, rust, javascript, typescript, java, kotlin, go, c, cpp, ruby, swift, csharp

## Workflow

1. Use `apex_doctor` to verify prerequisites for the target language
2. Use `apex_run` to get coverage gap report
3. Write tests targeting uncovered branches
4. Use `apex_detect` to find security issues
5. Use `apex_ratchet` in CI to enforce coverage standards
