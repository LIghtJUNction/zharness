# MCP protocol support

zharness is a Sans-I/O component library. It does not open sockets, spawn
processes, or implement an HTTP server. The application supplies complete
JSON-RPC messages and caller-owned input/output storage.

## Protocol revisions

The current stable MCP revision is **2026-07-28**. It is a stateless protocol
era: each request carries its version and client capabilities in `params._meta`,
and servers advertise support through the mandatory `server/discover` method.
The former `initialize` / `notifications/initialized` handshake and
protocol-level sessions belong to the legacy era (through 2025-11-25).

References:
- [Current specification and versioning](https://modelcontextprotocol.io/specification/2026-07-28)
- [Discovery](https://modelcontextprotocol.io/specification/2026-07-28/server/discover)
- [Lifecycle and legacy negotiation](https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle)
- [2026-07-28 changelog](https://modelcontextprotocol.io/specification/2026-07-28/changelog)

## Important implementation status

The current server implementation is an early bounded JSON-RPC tools subset.
It is **not yet a complete implementation of MCP 2026-07-28**. In particular,
full per-request metadata/version validation, discovery response semantics,
resultType, cache hints, subscriptions/listen, modern transport requirements,
and the full tools schema/result surface still require implementation and
conformance tests. The legacy initialize response must not be mistaken for
modern negotiation.

The library intentionally does not claim complete JSON Schema validation.
Tool descriptors carry caller-authored schema JSON; the application remains
responsible for authorization and domain validation.
