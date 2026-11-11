# MCP extraction

MCP protocol code now lives in the independent repository
https://github.com/LIghtJUNction/zmcp . zharness no longer exports an MCP server,
JSON-RPC parser or transport codec. It retains exactly one direct dependency:
zcall = 0.1.1 with default features disabled.

An application combines both libraries. Match zmcp::mcp::Action::CallTool, decode
the borrowed tool name into caller scratch, and create a zharness Invocation with
the arguments bytes. Validate and authorize, then execute. Return the retained
text and error flag through zmcp::Server::tool_result. MCP request IDs belong to
MCP; zcall tool-call IDs belong to the model turn. Never replace one with the
other merely because both are called IDs.

Use zmcp without zharness for a standalone MCP service. Use zharness without zmcp
for an embedded tool dispatcher. This avoids pulling model protocols into every
MCP service and avoids a second direct dependency in zharness.

Registry name collision currently blocks installing the new zmcp from crates.io.
Its existing 0.0.1 package must not be mistaken for this implementation. A runnable
composition project will pin the verified package once its publication name is
resolved; do not introduce a path or Git dependency into zharness's release.
