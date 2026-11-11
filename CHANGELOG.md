# Changelog

## Unreleased — 0.2.0

- Remove the MCP integration workspace and all sibling-repository CI checkouts.
- Add native caller-buffered Context groups, token budgets, pinned retention,
  whole-group eviction, explicit summary compaction, forks and zcall request building.
- Add a ToolLoop action state machine with actual ToolReady retention, round/call
  limits, hook-gated dispatch, parallel completion tickets and cancellation.
- Add tuple-composable before/after hooks with explicit allow, pause and deny semantics.
- Add capability-matched task DAGs, fenced leases, handoff/renewal/expiry, dependency
  joins, failure propagation and bounded mailboxes for application-owned agent runs.
- Add standalone context, hook, loop and cluster examples; real stream replay and
  randomized accounting tests; allocation-counted native component benchmarks.
- Keep zcall =0.1.2 as the sole direct dependency and preserve no_std core boundaries.
- No registry publication is performed by the source update.

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
