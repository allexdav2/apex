---
date: 2026-03-14
crew: integrator-officer
affected_partners: [security-detect, runtime]
severity: major
acknowledged_by: []
---

## Two detectors silently dead — in default_enabled() but not wired in from_config()

`MissingTimeoutDetector` and `SessionSecurityDetector` are declared in `mod.rs`,
re-exported, listed in `default_enabled()`, but have no matching branch in
`pipeline.rs from_config()`. They never run. Tests encode the wrong detector
count (18/17 instead of 20/19).

Files:
- `crates/apex-detect/src/pipeline.rs` (missing branches + wrong test assertions)
- `crates/apex-detect/src/config.rs:12-13` (lists them as enabled)

`BanditRuleDetector` is also unreachable via config but may be intentional (opt-in).
