# zcall bridge

The bridge must consume only zcall's public protocol-independent API. It must
not inspect OpenAI, Anthropic, Gemini or other provider JSON.

## Current public boundary

tools::Invocation::from_zcall converts a public zcall::v1::ReadyTool into a
borrowed view containing the original tool-call ID, tool name and complete
validated JSON object argument bytes. No argument copy or allocation is needed.

## Remaining integration work

This is not yet the complete bridge requested for zharness. The current crate
does not yet provide a tested end-to-end adapter that consumes Session events,
dispatches multiple interleaved ToolReady values, constructs matching tool
results through zcall's RequestBuilder, and prepares the next model turn.
Do not treat Invocation alone as that full integration.

The next implementation must be derived from the published zcall 0.1.1 API,
including the exact result-submission contract, and must preserve each
provider-independent call ID. It must not synthesize IDs or execute incomplete
arguments.
