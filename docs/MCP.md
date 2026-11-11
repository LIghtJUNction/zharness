# MCP extraction

MCP protocol code lives in the independent repository
https://github.com/LIghtJUNction/zmcp . zharness no longer exports an MCP server,
JSON-RPC parser or transport codec. Its sole direct published dependency is
zcall = 0.1.2, with default features disabled.

The runnable integration/mcp application composes both libraries. Match
zmcp::mcp::Action::CallTool, decode its tool name into caller scratch, validate
through zharness, explicitly authorize the call and invoke an Executor. Return
the retained text and error flag through zmcp::Server::tool_result.

MCP request IDs belong to MCP; zcall tool-call IDs belong to model turns. Never
replace one with the other because both happen to be called IDs. The separate
zcall_bridge example demonstrates the genuine model stream and next-turn flow.

Use zmcp alone for an MCP service and zharness alone for model-tool dispatch.
This avoids pulling model-protocol code into an unrelated MCP service and avoids
a second direct dependency in zharness. Applications pay for the capabilities
they actually compose.

The integration workspace is publish=false and excluded from cargo package. It
pins a verified zmcp Git checkout by SHA in CI and uses local source paths only
inside that unpublished application. Formal zharness dependencies remain exact
registry versions. Both libraries validate their own boundaries, so the example
explicitly validates the tool arguments again through zcall to create zharness's
validated type and apply its limits; it does not claim a single-parse fast path.

The crates.io name zmcp is occupied by evolutionengine's 0.0.1 package. The new
repository must not be confused with that release. Registry publication requires
an available package name or an actual ownership transfer; none is fabricated.
