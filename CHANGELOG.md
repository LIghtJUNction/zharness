# Changelog

## Unreleased — 0.1.0 candidate

- Extract MCP protocol and transport handling to independent zmcp.
- Preserve zcall 0.1.1 as the sole direct published dependency.
- Add bounded static/fixed registries and optional caller-owned sorted indexes.
- Reuse zcall JSON validation with explicit string/integer resource policy.
- Add ToolReady retention, validated execution, failed results and result submission.
- Execute the real interleaved bridge example as an integration test.
- Strengthen dependency, no_std, WASM, package and benchmark validation.
