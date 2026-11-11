# Changelog

## 0.2.1 — 2026-11-11

- Add caller-buffered ToolResultTransform and Completion transform helpers that
  preserve real call IDs and original error flags before history commitment.
- Add backward-compatible default before_result/after_result hook methods with
  ordered tuple/reference forwarding and explicit allow/pause/deny handling.
- Add zero-copy Identity, Trim, adjacent-line deduplication, marked UTF-8-safe
  truncation, validated JSON minification and caller-buffered transform chains.
- Validate callback lengths/UTF-8; preserve original results on processing failure;
  never retry the original tool or fall back to unfiltered output automatically.
- Add a runnable reduction example, real interleaved-session and parallel-buffer
  tests, and 25 allocation-counted output benchmark cases.
- Publish the native harness work from the previously unreleased 0.2.0 baseline.
- Keep zcall =0.1.2 as the only direct dependency; no MCP integration is restored.

## 0.2.0 — unpublished development baseline

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
