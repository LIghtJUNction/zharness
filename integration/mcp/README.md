# Application-level composition

This unpublished integration workspace uses local source paths and is excluded
from the zharness workspace and cargo package. The published library still has
only the exact registry zcall dependency. Keep the two repositories as siblings,
not one workspace nested inside the other:

```text
checkout/
  zharness/
    integration/mcp/
  zmcp/
```

From a fresh parent directory:

```sh
git clone https://github.com/LIghtJUNction/zharness
git clone https://github.com/LIghtJUNction/zmcp
git -C zmcp checkout 010fb64b0c73e8775f2828eb220f5eb1128a3af4
cargo run --locked --manifest-path zharness/integration/mcp/Cargo.toml
```

The application owns the registry and allowlist. zmcp classifies a modern MCP
tools/call, the application validates through zharness and invokes its executor,
then zmcp encodes the retained text. MCP IDs are never confused with model-tool
IDs. The root zcall_bridge example separately covers an actual interleaved model
stream and subsequent model request.

Independent libraries validate their own boundary. This composition deliberately
validates arguments again through zcall to create zharness's private validated
type and apply its limits. That cost is explicit, not hidden behind an unchecked
constructor or a claimed single-parse fast path.
