# Architecture

zharness supplies native harness state transitions while the application owns
all external work. zcall remains authoritative for model protocols, streaming
aggregation, request/response IR and model-turn lifecycle. zharness uses only
zcall's public v1 interfaces; it never interprets provider-specific tool fields.

```text
application: transport, authorization, model selection, clocks and scheduling
  |
  +-- Context ----> zcall RequestBuilder / Request ----> application transport
  |                         ^
  +-- ToolLoop <---- zcall Session events
  |       |
  |       +-- Execute ticket --> registry / validation / application executor
  |                                  |
  |                         retained Completion + explicit history commit
  |
  +-- Hooks: before gates / after observations at state-transition boundaries
  |
  +-- TaskBoard + Mailbox: multiple application-driven agent runs
```

Context borrows message groups and their payloads. Whole-group eviction avoids
fragmenting a correctly grouped tool exchange. Forks copy only descriptors into
caller storage. Token cost is a caller contract: a model-specific tokenizer or
explicit estimate includes framing and extra request data. The library neither
invents tokenizer accuracy nor invokes a summarizer during compaction.

ToolLoop separates streaming observations from side-effecting tool dispatch.
ToolReady data is retained in fixed CallSlot storage. The terminal outcome gates
execution; incomplete/upstream-failed turns do not automatically execute tools.
Execute tickets are scoped to run, round and slot. The application retains calls
and outputs into its own history before the controller releases per-turn storage.

Hook composition uses generics and tuples, not a mandatory dynamic registry.
Before gates can pause or reject a transition; after observers see committed
state. This avoids returning a retry signal after an irreversible tool has run.
Hook failures/panics and external effects are still application responsibilities.

TaskBoard holds task descriptors, not agent runtimes. Dependencies are submitted
in topological order; claims match capability masks and allow one active lease
per agent. Epoch, attempt and expiry fields fence stale completions. Explicit
expiry requeues work only when the application requests it. Network delivery,
worker startup, durable storage and distributed agreement remain outside the core.

All modules can be used separately. The dependency closure remains
zharness -> zcall 0.1.2 -> zconv 0.2.2. No MCP source, integration workspace or
MCP repository checkout is required for building, testing or publishing zharness.
