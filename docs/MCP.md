# MCP boundary

MCP is developed independently in https://github.com/LIghtJUNction/zmcpio .
zharness contains no MCP integration workspace, protocol parser, transport
adapter, dependency or cross-repository CI checkout. Its sole direct dependency
remains the published zcall =0.1.2 with default features disabled.

An application may compose its chosen MCP client/server with zharness, but that
application and the independent MCP repository own the integration contract.
MCP request IDs and model tool-call IDs are separate identities; the application
must correlate them explicitly rather than substituting one for the other.

This repository focuses on native harness primitives: bounded context retention,
explicit model/tool turns, lifecycle hooks and application-driven multi-agent
coordination. None of these primitives require MCP or execute network I/O.
