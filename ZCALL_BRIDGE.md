# zcall bridge

zharness consumes only zcall's public protocol-independent API. It never
inspects OpenAI, Anthropic, Gemini or other provider JSON.

## ToolReady conversion

tools::Invocation::from_zcall converts a complete public
zcall::v1::ReadyTool into a borrowed view containing the original call ID,
registered tool name and complete JSON object argument bytes. The view does not
copy or allocate. Incomplete tool arguments never become executable: zcall
emits ToolReady only after the complete argument document has been validated.

## Result lifecycle

The bridge example exercises the actual public lifecycle:

1. Construct caller-owned zcall StreamWorkspace and Session.
2. Feed arbitrarily chunked provider stream bytes to Session::step.
3. Observe EventData::ToolReady and convert it to Invocation.
4. Check registry membership and call the application-supplied Executor.
5. After the terminal event, submit each completed call ID through
   Session::submit_tool_result.
6. Build a follow-up zcall RequestBuilder request containing the original
   assistant tool-call blocks and matching results through
   Request::append_tool_result.
7. Encode the next model request through zcall's protocol-independent API.

Run it with cargo run --example zcall_bridge.

## Boundaries

zharness does not choose a model, make network requests, execute an agent loop,
authorize a tool, or retain conversation history. The application controls
parallelism and decides how tool failures are represented in its next turn.
The bridge preserves original call IDs and does not invent replacements.
