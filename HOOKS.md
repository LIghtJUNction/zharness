# Lifecycle hooks

Implement Hooks and pass the implementation into hooked operations. The library
stores no global hooks. A tuple such as (audit, policy) composes different hook
types without a required Box, dynamic trait object or allocation. Nested tuples
extend the chain. The caller owns callback state and lifetime.

Before events describe context changes, model actions, tool dispatches and agent
assignments. Return Decision::Allow to permit the transition, Pause to defer it,
or Deny to return Error::Denied. A tuple evaluates before hooks left-to-right
and short-circuits at the first non-Allow result. Library state is not mutated
before all participating gates allow the proposal.

After events observe committed context, model outcomes, tool results, stops,
assignments, task completions, expired leases and cancellations. All tuple
observers run in order and return no decision. A completed side effect must not
be represented as a fresh retryable proposal just because an observer failed.
Callbacks are application code: keep them bounded and do not panic across a
boundary where partial application-side changes would be unsafe.

## Tool-result processing hooks

Completion::transform_with_hooks adds a separate processing boundary after tool
execution and before history/session commitment. before_result receives read-only
ResultMeta (tool name, call ID, is_error) and original text. Pause leaves the raw
result available and skips processing; Deny returns Error::Denied. A transform
must return valid UTF-8 within caller output bounds before after_result sees it.
The observer receives original byte length and processed text, but cannot veto.

These are default trait methods, not new variants in the existing Before/After
enums. Existing Hooks implementations and exhaustive lifecycle-event matches keep
compiling. Tuple and mutable-reference forwarding apply to the new methods too.
After::ToolFinished retains its existing meaning: an accepted ToolLoop result.
after_result means successful processing only; an explicit application retry may
produce another processing observation before a single eventual loop commit.

Use ToolResultTransform for the actual text policy; use Chain with supplied scratch
for multiple stages. No fallback to unfiltered raw output occurs on a chain error.
The returned Completion preserves call ID and is_error. Read [OUTPUT.md](OUTPUT.md)
for UTF-8, structured data, parallel buffers and semantic-safety contracts.

## Pause and authorization

Pause may cause the same before event to be evaluated again. Before callbacks
must not perform the proposed tool/model action. Store a decision or query an
already-available policy result instead. Remote approval or model-generated
summary work can be scheduled by the application after a Pause, then processing
resumes with retained input. The library has no asynchronous approval runtime.

Hooks are explicit control points, not automatic authorization. NoHooks allows
all proposals. Direct unhooked Context/Completion methods deliberately bypass
callbacks. Task capability masks select eligible workers but do not grant
permission to perform real external operations. Keep authorization at execution.

Run `cargo run --locked --example hooks` and `cargo run --locked --example tool_output`.
Tests cover ordering, short-circuiting, unchanged state on pause/deny, successful
processing observations and separate loop completion observations.
