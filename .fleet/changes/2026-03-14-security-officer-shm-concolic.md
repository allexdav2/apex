---
date: 2026-03-14
crew: security-officer
affected_partners: [runtime, platform]
severity: minor
acknowledged_by: []
---

## ShmBitmap unsafe Send+Sync overpromises; concolic tracer writes into target dir

1. `ShmBitmap` (shm.rs:23-24) implements unsafe Send+Sync for a raw pointer type.
   Currently safe by caller convention but Sync is overpromising.
2. `PythonConcolicStrategy` writes `.apex_tracer.py` and `.apex_trace.json` into
   `target_root` — pollutes user repo and has TOCTOU race window.
