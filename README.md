# zharness

**Small, composable Rust building blocks for your own AI harness.**

zharness depends on [zcall](https://github.com/LIghtJUNction/zcall) for the
protocol-independent model-call contract. It adds MCP (Model Context Protocol)
JSON-RPC primitives and a bounded tool registry/dispatcher. It is deliberately
not a complete harness: no agent loop, model routing, conversation database,
policy engine, scheduler, HTTP client, or opinionated runtime is included.

## Design

- **Atomic capabilities:** MCP message framing, JSON-RPC request/response
  encoding, server initialization, tool listing, tool invocation and zcall
  tool-event bridging are separate pieces.
- **Caller-owned memory:** protocol operations use caller-provided input and
  output slices. No internal unbounded buffers or hidden allocation.
- **Bring your own transport:** stdio, HTTP, WebSocket and in-process transports
  can drive the same MCP message API. The optional std feature only supplies
  convenience I/O adapters.
- **Bring your own execution:** tools are static descriptors plus a caller
  implementation. zharness never executes arbitrary commands or grants
  permissions on its own.
- **zcall stays authoritative:** model request/response semantics and streaming
  tool-call validation come from zcall. MCP tool schemas and execution are
  separate.

## Dependency policy

The published library has exactly one direct dependency: zcall = 0.1.1,
with default features disabled. No async runtime, HTTP stack, serde, JSON
framework, or platform-specific dependency is pulled in. The protocol core
uses bounded slices and a small JSON scanner. std is an optional feature
(default enabled for examples); --no-default-features builds the core without
the standard library.

## Quick start

```toml
[dependencies]
zharness = "0.1"
```

See:
- [basic tool registry](examples/basic.rs)
- [MCP stdio server](examples/mcp_stdio.rs)
- [MCP protocol notes](docs/MCP.md)
- [performance methodology](docs/PERFORMANCE.md)

## Performance

The target is microsecond-scale local dispatch for small MCP messages, not a
blanket latency guarantee. Tool execution, transport, operating-system
scheduling and model inference are outside the library's control. Run
cargo bench on the target hardware before making performance claims.

## Build

```sh
cargo test --all-targets --locked
cargo test --no-default-features --lib --locked
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
```

License: MIT OR Apache-2.0.
