# Architecture

The application owns its agent loop. zharness owns only local tool composition;
zcall owns model formats, streaming aggregation, lifecycle and request encoding;
zconv remains zcall's private protocol implementation dependency. MCP wire
handling is a sibling library, zmcp, composed by the application.

The bridge's only entry from model streaming is EventData::ToolReady. ReadyTool
already has complete object arguments. zharness applies its explicit resource and
business policy before invoking an executor. Its validation pass reuses the public
zcall Json API, not zconv internals and not a copied provider parser.

InvocationStorage is the scheduling boundary. Copy into it only when a call must
outlive an event callback. Callback acceptance and execution are separate: a
Control::Pause path must not execute a side effect. The application chooses which
retained calls run concurrently and owns cancellation and retry policy.

Completion retains the true tool-call ID and caller-owned output. Append it only
after the assistant call exists in the next RequestBuilder history. Submit it to
the Session only after the application has accepted the retained result. These
are separate operations rather than an unsafe hidden distributed transaction.
