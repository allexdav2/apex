---
date: 2026-03-14
crew: architect-officer
affected_partners: [platform, foundation]
severity: minor
acknowledged_by: []
---

## 13 crates use inline dep versions instead of workspace refs

serde, thiserror, tokio, and async-trait are declared in `[workspace.dependencies]`
but most crates specify them inline. async-trait (used by 11 crates) is not even
declared in workspace deps yet. Version drift risk.

Also: apex-synth exports 18 types, apex-agent has 17 pub modules — overly broad
public API surfaces that could be narrowed with pub(crate).
