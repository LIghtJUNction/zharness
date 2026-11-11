# Application-level composition

This unpublished integration workspace deliberately uses local source paths. It
is excluded from the zharness workspace and cargo package. The published library
still depends only on the exact registry version of zcall.

From the repository root:

```sh
git clone https://github.com/LIghtJUNction/zmcp integration/mcp/zmcp
git -C integration/mcp/zmcp checkout 010fb64b0c73e8775f2828eb220f5eb1128a3af4
cargo run --locked --manifest-path integration/mcp/Cargo.toml
```

The application owns the registry and allowlist. zmcp classifies a modern
MCP tools/call, the application validates through zharness and invokes its
executor, then zmcp encodes the retained text. MCP IDs are never confused with
model-tool IDs. zcall ToolReady integration is demonstrated separately by the
root zcall_bridge example with an actual interleaved model stream.

The independent libraries validate their own boundary. Consequently this
composition validates argument JSON again through zcall to create zharness's
private validated type and apply its limits. That cost is explicit, not hidden
behind an unchecked constructor or a claimed single-parse benchmark.
