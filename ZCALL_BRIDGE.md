# zcall bridge

1. Build the first turn using zcall RequestBuilder and Tool::as_zcall.
2. Drive zcall Session with the application's transport bytes.
3. Match Invocation::from_event; only ToolReady returns Some.
4. Retain into InvocationStorage if the event callback will return.
5. Prepare against a Registry and explicit ArgumentLimits; run application policy.
6. Execute into a caller buffer; retain Completion.
7. Append Prepared::content through RequestBuilder's assistant message.
8. Append Completion through Request::append_tool_result, then acknowledge the
   retained result through Session::submit_tool_result.

No next model call is started automatically. Multiple calls use distinct storage
and original IDs; completion order is independent of streaming order. Unknown or
repeated result IDs are rejected by zcall. Failed results keep is_error=true in
the zcall IR. A target protocol unable to preserve that semantic must be handled
through zcall's explicit conversion policy; zharness never silently drops it.

The runnable examples/zcall_bridge.rs is also executed by an integration test.
It uses published zcall 0.1.2 rather than a Git/path override or a fake event type.
