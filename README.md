# zharness

Small, caller-buffered building blocks for a developer-owned AI harness.
The core is no_std and forbids unsafe Rust. Its only direct dependency is the
published `zcall = 0.1.1`, with default features disabled.

MCP wire handling has been extracted to [zmcp](https://github.com/LIghtJUNction/zmcp).
The application composes the two; neither library depends on the other.
zharness does not implement an agent loop, network client, provider parser,
authorization policy, scheduler or persistence.

## Components

`tools` provides static and caller-owned fixed registries, optional sorted indexes,
validated tool schemas and a synchronous executor contract. `validation` reuses
zcall's public JSON parser and applies bounded string/integer policy. `bridge`
adapts only zcall ToolReady events, retains calls in fixed storage when needed,
checks arguments before dispatch and appends results through zcall's public API.

```rust
use zharness::{Tool, ToolSet};
static TOOLS: &[Tool<'static>] = &[
    Tool::new("echo", "Return validated input", r#"{"type":"object"}"#),
];
let tools = ToolSet::new(TOOLS)?;
assert!(tools.get("echo").is_some());
# Ok::<(), zharness::Error>(())
```

## Run

```sh
cargo test --locked --all-targets
cargo run --locked --example basic
cargo run --locked --example tool_registry
cargo run --locked --example tool_execution
cargo run --locked --example zcall_bridge
cargo bench --locked --bench dispatch
```

The bridge example constructs an initial request, consumes a real interleaved
zcall SSE fixture in 17-byte chunks, retains two ToolReady calls, validates and
executes them in reverse completion order, submits real IDs, and encodes the next
request. No live credentials or provider requests are needed for the example.

## Contracts

JSON syntax is not authorization or JSON Schema conformance. Application policy
must run before side effects. Arguments supplied to Executor are complete validated
objects; a caller cannot construct that wrapper from arbitrary bytes. Business
failures are retained ToolOutput values with is_error=true. Capacity errors never
cause automatic execution retries. Partial or unrecognized model events are not
converted into calls.

Use linear lookup for small tables or an explicit caller-owned sorted index for
larger tables. No hash map or allocation is introduced automatically. The library
never allocates; test/benchmark fixture generation can use std outside the core.

See [architecture](ARCHITECTURE.md), [tools](TOOLS.md), [bridge](ZCALL_BRIDGE.md),
[security](SECURITY.md), [MCP extraction](docs/MCP.md) and
[performance](PERFORMANCE.md). This is an unreleased candidate, not a claim of a
completed production audit or a universal microsecond latency guarantee.
