# Architecture

The application owns external work. zharness supplies bounded state transitions;
zcall owns model protocols, IR, streaming aggregation and lifecycle. All zcall
integration uses public v1 APIs. No provider-specific fields or MCP source are
parsed by the harness components.

```text
application: UI, transport, permissions, model capabilities, clocks and storage
  |
  +-- Conversation / ModelSettings: gated local acceptance and configuration
  |        |
  +-- Context -> zcall Request -> explicit encode/send operation -> Sender
  |                                      |
  +-- ResponseStream <--- zcall Session events ---> ToolLoop
  |       |                                          |
  |       +-- final assistant acceptance              +-- execution tickets
  |                                                       |
  +-- old native Hooks <--- Executor / Command / output transforms
  |
  +-- TaskBoard / Mailbox: local coordination for application-owned agent runs
  |
  +-- typed Lifecycle<G, O>: gated/observed application operations and receipts
```

Context retains borrowed message groups, caller token costs and explicit summaries.
Whole-group eviction preserves correctly grouped tool exchanges. Forks copy only
descriptors. Token estimates, semantic summaries and payload lifetime remain caller
contracts; the core does not create a tokenizer, storage system or summarizer.

ToolLoop separates stream observation from execution. Genuine ToolReady events
are retained in fixed slots, and the terminal WaitingTools outcome permits dispatch.
The application owns concurrent work and retains results/history before clearing
per-turn storage. Original call IDs and run/round tickets reject mismatched or
stale completions without a hidden cross-system transaction.

Existing Hooks methods gate/observe native operations without changing their
semantics. Typed lifecycle channels extend coverage independently rather than
appending dozens of methods to that trait. Gate controls proposals; Interceptor
edits a copy before validation/gating; Observer records metadata by default.
The non-cloneable Span receipt supports explicitly bracketed async work without
requiring Futures, threads or a runtime in the core. All identities and retained
receipt storage are supplied/owned by the caller.

Conversation and ModelSettings make their local state changes only on accepted
operations. These do not globally observe UI changes or reconfigure in-flight
model calls. Sender acceptance is adapter-defined, not proof of server receipt.
ResponseStream receives original zcall events and honors event rejection/replay;
first visible output, terminal outcome and final message acceptance remain
separate. WaitingTools and failures cannot be finalized as successful replies.

TaskBoard stores descriptors, not agent workers. Topological dependencies, one
active lease per agent, attempt/epoch fencing and explicit expiry give a bounded
coordination layer. Leases do not revoke already-started external effects or
provide distributed consensus/exactly-once delivery. Mailboxes need caller-owned
synchronization and transport when used across threads or machines.

Lifecycle::run can surround prompt construction, database writes, checkpoints,
policy checks, retries and worker operations. A label/notification alone performs
none of those tasks. Native wrappers and explicitly application-wrapped boundaries
are enumerated separately in LIFECYCLE.md. Error propagation never implies rollback
of external effects or permission for a blind retry. Panics are not caught.

The dependency graph remains zharness -> zcall 0.1.2 -> zconv 0.2.2. The source
change does not publish a new crate, add a runtime or depend on the MCP repository.
