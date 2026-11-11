# Security and ownership

Model output, tool arguments, metadata, summaries, task payloads and agent messages
are untrusted. Registered names, valid JSON, pinned context and capability bits
are not authorization decisions. The application validates schema/business rules
and approves dangerous operations at the actual execution boundary.

The library is no_std and forbids unsafe code. It uses caller-owned fixed buffers
and borrowed data, not hidden growing caches. Configure token, byte, object-field,
string, depth, group, tool-call, round and task limits for the application. Core
capacity failures do not automatically allocate or retry effects. Generic hooks,
executors and payload types can independently allocate; audit those separately.

Context token counts are supplied by the application and may be estimates. Reserve
for output and request overhead, and recompute for the target model. An incorrect
cost cannot be turned into a hard model-context guarantee. Keep call/result groups
whole during eviction. Pin trusted instructions, and do not elevate untrusted
summaries into trusted instructions without an application decision.

ToolReady is accepted only after zcall completes argument validation. The native
loop additionally waits for WaitingTools before scheduling work. A paused stream
event can replay: return Continue only after retention succeeds, and never execute
a side effect then return Pause. Tests exercise buffer growth and replay against
a real zcall Session. Late/duplicate results are rejected by dispatch tickets and
real call-ID checks. Keep history and output buffers alive until consumers finish.

Before hooks may run repeatedly and must not execute the proposed work. After
observers see committed state and cannot roll it back. Hooks are not a sandbox;
the application must not allow callback panic or blocking work to break its own
consistency or latency contract. No implicit permission grant accompanies NoHooks.

Cancellation signals state, not external rollback. Lease expiry, handoff and
renewal fence only coordinator completions. Use monotonic coordinator time, fresh
run IDs/board epochs and external idempotency/fencing before retrying real effects.
Neither task payloads nor mailbox sender IDs authenticate a remote peer. Networking,
encryption, access control, durable recovery and distributed agreement are outside
this crate. Mailbox and TaskBoard access must be serialized by the application.

Never re-execute a side-effecting tool merely to enlarge a result-encoding buffer.
Retain its result and retry only the failed serialization. Clear operations release
logical state; they do not securely erase every byte of retained storage.
