# Changelog

## 0.1.1 — 2026-11-11

- Upgrade the only direct dependency to published `zcall =0.1.2` (from 0.1.1).
- Refresh the transitive `zconv` lock to published 0.2.2 (from 0.2.0).
- Keep `no_std`, caller-owned storage, and the `zcall::v1` ToolReady bridge unchanged.
- Require the exact registry dependency closure in CI and test the versioned release path.

## 0.1.0 — unpublished development baseline

- Extract MCP protocol and transport handling to independent zmcp.
- Preserve zcall 0.1.1 as the sole direct published dependency.
- Add bounded static/fixed registries and optional caller-owned sorted indexes.
- Reuse zcall JSON validation with explicit string/integer resource policy.
- Add ToolReady retention, validated execution, failed results and result submission.
- Execute the real interleaved bridge example as an integration test.
- Strengthen dependency, no_std, WASM, package and benchmark validation.
