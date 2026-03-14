# APEX TODO

## Threat-Model-Aware Detection

Current detectors have ~97% false positive rate on APEX itself because they don't know the software's trust boundaries. `Command::new("cargo")` in a CLI tool is not command injection.

- [ ] Add `[threat_model]` section to `apex.toml` — `type = "cli-tool" | "web-service" | "library" | "ci-pipeline"`
- [ ] Classify sources by trust level per threat model (e.g. `sys.argv` trusted in CLI, untrusted in web service)
- [ ] Wire CPG taint analysis into detectors — only flag flows from **untrusted** sources to sinks
- [ ] Suppress pattern-match findings when no taint flow from untrusted source exists
- [ ] Add `--threat-model` CLI flag to `apex audit`

## JS/TS Concolic — Not Yet Covered

- [ ] Dynamic `eval()` / `new Function()` — not statically analyzable, needs runtime tracing
- [ ] Proxy/Reflect metaprogramming — intercepted property access creates invisible branches
- [ ] Async control flow constraints — Promise branching, `await` paths, race conditions
