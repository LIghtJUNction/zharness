# Tool output processing

A tool result can be reduced before it enters model history. `Completion::transform`
and `transform_with_hooks` accept a `ToolResultTransform`, immutable tool metadata
and caller-owned output. They preserve the real call ID and original `is_error`
flag. They never run the tool, append history, acknowledge a Session or complete
a ToolLoop ticket on the application's behalf.

```text
validated/authorized tool execution -> retain original Completion
  -> Hooks::before_result (allow / pause / deny)
  -> ToolResultTransform (one stage or a caller-buffered Chain)
  -> validate output length and UTF-8
  -> Hooks::after_result (observe the validated text)
  -> retain reduced history -> submit to zcall -> ToolLoop::commit_result
```

## Minimal use

```rust
use zharness::{bridge::Completion, hooks::{HookResult, NoHooks}};
use zharness::output::{Chain, DedupAdjacentLines, Truncate};

let original = Completion {
    call_id: "real-call-id",
    text: "error: missing config\nerror: missing config\n",
    is_error: true,
};
let mut scratch = [0; 1024];
let mut output = [0; 256];
let mut pipeline = Chain::new(
    DedupAdjacentLines,
    Truncate::new(256, "\n...[truncated]")?,
    &mut scratch,
);
let HookResult::Applied(reduced) = original.transform_with_hooks(
    "build", &mut pipeline, &mut output, &mut NoHooks,
)? else { unreachable!("NoHooks never pauses"); };
assert_eq!(reduced.text, "error: missing config\n");
assert_eq!(reduced.call_id, original.call_id);
assert!(reduced.is_error);
# Ok::<(), zharness::Error>(())
```

Run `cargo run --locked --example tool_output` for a complete executor, custom
DEBUG-line filter, result hooks, error-preserving context construction and JSON
minification. `tests/output_flow.rs` also drives genuine interleaved zcall events,
transforms two results, submits them out of order and advances the model turn.

## Processing primitives

`Identity` returns `Transform::Unchanged`: no copy and no output capacity needed.
`Trim` removes leading/trailing Unicode whitespace. `DedupAdjacentLines` removes
only adjacent byte-identical lines, including identical line endings. These are
opt-in text policies: repeated messages or surrounding whitespace can be meaningful.

`Truncate::new(max_bytes, marker)` limits plain text by UTF-8 bytes, including a
required nonempty truncation marker. It never splits a UTF-8 code point. It can
still cut grapheme clusters or important facts. This is not a token budget and
must not be used to produce a shortened-but-still-structured JSON document.

`JsonMinify::new(&mut tokens, limits)` first validates complete JSON with zcall's
public parser and caller-provided TokenStorage. It removes only insignificant
JSON whitespace outside strings. String escapes, numeric spelling and key/array
order remain unchanged. Malformed or resource-exceeding input fails; no unchecked
fast path bypasses validation. Minification does not enforce a JSON Schema or
remove fields. A schema-aware projector should be a custom transform that
re-encodes valid JSON and checks the projected contract.

`Chain::new(first, second, scratch)` validates intermediate output, runs stages in
order and stops on the first failure. Nest chains for more stages. An unchanged
second stage cannot return a borrow into temporary scratch: the chain copies that
intermediate text into the final caller output. If every stage is unchanged,
the original text is reused without copying. Stages may expand output; allocate
scratch for the largest intermediate, not just the expected final summary.

## Hooks and custom transforms

Implement `ToolResultTransform::transform(meta, input, output)` and return either
`Unchanged` or `Written(n)`. The library validates n and UTF-8 before creating a
Completion or passing the text to the next stage. Metadata is read-only input;
there is no return field that can silently substitute a call ID or change failure
to success. Custom transforms remain trusted application code, not a sandbox.

`Hooks::before_result` sees tool name, call ID, error status and original text.
`Pause` returns `HookResult::Paused` without calling a transform or touching its
output. `Deny` returns `Error::Denied`. `after_result` observes successful output
and original byte length; it cannot veto. Tuple hooks short-circuit before and
observe after in left-to-right order. Existing lifecycle enums are unchanged and
new trait methods have defaults, preserving existing hook implementations.

Result hooks do not replace `Before::Tool` or `After::ToolFinished`. The latter
still means the tool-loop result was committed. A result observer can run again
if the application explicitly repeats a successful transformation; do not count
it as an exactly-once side effect or a history commit.

## Failure, async work and parallel execution

Keep raw output until processing succeeds. On capacity/validation failure,
discard partial target/scratch bytes and retry only the transform, with retained
input and larger storage or a corrected policy. No automatic fallback exposes raw
output when a redaction/filtering stage fails. Before/transform callbacks must not
re-execute the original tool. A successful pipeline is not an atomic transaction
with history persistence or external effects; the application commits each layer.

For model-generated summaries or remote approval, pause and let the application
schedule that work. Retain the original result and any required call data. When
ready, supply an application transform using the saved summary, or construct an
explicit replacement Completion with the same identity/status. There is no hidden
model call, async runtime, network transport or allocation in these primitives.

Concurrent tools use separate raw/output/scratch buffers and per-task callback
state, or caller-managed synchronization. Return reduced results to one serialized
coordinator for history, Session and ToolLoop commitment. Scoped-thread tests
verify independent buffers and metadata; the library does not spawn those threads.

## Semantics and accounting

Lossy reductions must preserve the facts needed for the next decision. Preserve
error causes, warnings, IDs, required schema fields and source references under
your tool-specific policy. Mark truncation/summarization; retain raw output or an
application-owned retrieval reference when auditability requires it. Raw output
must not be elevated into trusted instructions by a summarizer.

Recompute the model-specific token cost after reduction before inserting the
message Group. Bytes saved are not necessarily tokens saved. Result reduction
and `Context::compact_with_hooks` solve different problems: the former transforms
one completed tool output; the latter replaces caller-selected whole history
groups with a supplied summary. Neither API silently summarizes or repairs a
broken tool-call/result relationship.
