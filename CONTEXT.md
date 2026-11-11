# Context management

A Context stores caller-owned Group descriptors. Each group borrows a slice of
Message values, and each message borrows zcall Content blocks or a retained
Completion. This preserves rich zcall content instead of converting everything
to strings. Message::ToolResult uses zcall's checked append_tool_result API.

## Budget and retention

Budget::available is limit minus reserved, checked for underflow. Reserve space
for model output, tool schemas and non-history prompt overhead. Group.tokens must
come from the application's tokenizer or a clearly labelled estimate. Costs can
change with the target model or serialization; recompute/rebuild the context
when changing that accounting contract. No hard token-fit promise is made for
incorrect caller counts.

push rejects duplicate group IDs, empty message groups, insufficient capacity or
insufficient token budget. Overflow::Reject preserves all prior groups.
Overflow::EvictOldest removes the oldest unpinned groups until both storage and
budget permit insertion. It never removes part of a group. All checks occur
before mutation; failure leaves ordering, descriptors and accounting unchanged.

Pinned groups survive eviction and cannot be replaced by compact. Pinning is a
retention rule, not authorization: an application can still create a new Context
with different instructions. Never combine independent security principals in
one context merely because their content fits the same buffer.

## Tool history and compaction

Put an assistant message containing all parallel tool calls and every matching
tool result in one group. The library does not automatically infer conversation
boundaries or repair unrelated groups. Context::build delegates call/result ID
checks to zcall. Unknown and duplicate tool results fail rather than receiving
invented IDs. A build error exposes no partially constructed Request.

compact(range, summary) replaces a nonempty range of complete, unpinned groups
with one caller-supplied Group. The summary must fit the same budget and must not
reuse an ID outside the replaced range. The library does not call a model or
assert that the supplied summary preserves every original fact. The caller
chooses role, content and cost, then can gate the change with a hook.

## Fork and lifetime

fork_into copies descriptors into another supplied buffer. Later eviction or
compaction on either branch does not alter the other's descriptors. Borrowed
message data remains shared and immutable; this is not a deep payload clone.
Keep every borrowed string/content/result alive while a Context or built Request
uses it. Reuse output buffers only after those borrows end.

push_with_hooks and compact_with_hooks emit Before::Context, then
After::ContextChanged only after an accepted mutation. Pause and Deny preserve
context state. Unhooked methods are explicit opt-outs, not hidden policy checks.

Run `cargo run --locked --example context_window`. Accounting, failure atomicity,
pinned groups, fork isolation and tool-result construction are tested in
`tests/context.rs`; seeded randomized operations are in `tests/recovery.rs`.
