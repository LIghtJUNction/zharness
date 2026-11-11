# Multi-agent primitives

TaskBoard coordinates application-owned agent runs. It does not start model
sessions, create threads, spawn subprocesses or connect to remote machines. A
worker can drive its own Context and ToolLoop after the application accepts a
lease. The same board can be used locally or behind a caller-provided transport.

## Tasks and dependencies

Agent has an ID and capability bitmask. Task borrows a payload and a list of
prerequisite IDs. Every prerequisite must already be present on submission;
therefore insertion is topological and a cycle cannot be introduced. The board
rejects duplicate IDs/dependencies and capacity exhaustion before mutation.

claim chooses the first eligible slot whose prerequisites completed successfully
and whose requirement bits are present in the agent's capability mask. One agent
has at most one active lease. Idle means no compatible ready task exists; Busy
means the agent already owns a running lease. Ordering is deterministic slot
order, not a promise of priority scheduling or starvation-free fairness.

Independent tasks provide fan-out. A task depending on several others provides
fan-in; join reports Pending, Ready, Failed or Cancelled for an explicit set.
Failed or cancelled dependencies block their pending descendants. retire reclaims
terminal slots only when no remaining descriptor depends on them; retire leaves
before prerequisites. No unbounded completed-task archive is retained.

## Leases and handoff

Lease carries board epoch, task ID, agent ID, attempt and expiry. The application
supplies a fresh epoch after recreation and monotonic coordinator clock values.
Duration must be nonzero; deadline and attempt overflow fail explicitly. Task
completion is accepted only for the current, unexpired lease and only once.

renew replaces the lease value; its predecessor is immediately stale. handoff
moves work to an idle compatible agent with a new attempt, after hook approval.
expire explicitly requeues expired leases. No implicit retry occurs while merely
reading state or asking for a task. Cancellation fences later board completions.

These leases fence **board state**, not external resources. A worker can continue
an external operation after losing its lease. To safely retry writes, payments,
commands or other effects, use idempotency keys or resource-enforced fencing and
application-specific recovery. This library provides no distributed consensus,
crash persistence, authentication or cross-machine exactly-once guarantee.

## Mailboxes and ownership

Mailbox<T> stores messages in a caller-provided Option<T> slice. send returns the
original message on capacity failure. receive_matching removes the first match
and preserves the order of other messages. Envelope borrows payload bytes and
includes sender, recipient and correlation IDs; receive_for selects a recipient.

Mailbox is not a transport or a concurrent queue. Serialize access or apply your
chosen synchronization. Sender IDs are application data, not proof of identity.
Generic payloads can allocate on their own; the mailbox itself does not allocate.
Keeping a TaskResult or Envelope alive requires retaining its borrowed payload.

Run `cargo run --locked --example agent_cluster` for two workers and one joining
coordinator. Tests cover capability matching, dependencies, cancellation,
handoff, renewal, expiry, late results, ID reuse, clock/overflow checks, hook
pauses and mailbox backpressure.
