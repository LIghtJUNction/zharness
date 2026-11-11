# zharness

Native, caller-buffered primitives for a developer-owned AI harness: context
windows, model/tool turns, lifecycle hooks and multi-agent coordination. The
library is `no_std`, forbids unsafe code, and has one direct dependency:
`zcall = "=0.1.2"`, with default features disabled.

**Source version 0.2.0 is an unreleased candidate.** The published 0.1.1 package
contains the earlier tool registry/bridge, not the new native modules described
here. Updating the source version does not publish a crate.

## Boundary

The library maintains bounded state and returns actions. Your application performs
I/O, chooses models, approves tools and schedules work. No background worker,
HTTP client, automatic retry, tokenizer, summarizing model, database or distributed
consensus implementation is started implicitly. MCP is developed independently
in [zmcpio](https://github.com/LIghtJUNction/zmcpio); this repository has no MCP
integration directory or sibling-repository CI dependency.

| Module | Atomic capability |
|---|---|
| `context` | Grouped history, budgets, pinned instructions, eviction, explicit summaries, forks and zcall request construction |
| `turn` | Model/tool action state machine, actual ToolReady retention, bounded rounds/calls, cancellation and out-of-order completions |
| `hooks` | Ordered before gates and after observers, composed through tuples |
| `cluster` | Capability matching, task dependencies, leases, handoff, joins and bounded mailboxes |
| `tools`, `validation`, `bridge` | Tool definitions, execution interfaces, argument validation and public zcall result APIs |

## Context without an owned conversation heap

```rust
use zharness::context::{Budget, Context, Group, Message, Overflow};
use zharness::zcall::v1::{Content, Role};

let instructions = [Content::text("Use only approved tools.")];
let question = [Content::text("Review this change.")];
let system = [Message::new(Role::System, &instructions)];
let user = [Message::new(Role::User, &question)];
let mut slots = [None; 8];
let mut context = Context::new(
    &mut slots,
    Budget { limit: 4096, reserved: 1024 },
)?;
// Illustrative caller estimates. Use the target model's tokenizer in production.
context.push(Group::new(0, &system, 16).pinned(), Overflow::Reject)?;
context.push(Group::new(1, &user, 20), Overflow::EvictOldest)?;
assert_eq!(context.tokens(), 36);
# Ok::<(), zharness::Error>(())
```

A group is one eviction/compaction unit. Keep an assistant's parallel tool calls
and their results together. `fork_into` copies descriptors into another caller
buffer; message data stays borrowed. `compact` accepts an application-generated
summary and refuses to replace pinned groups. `Context::build` creates a request
through public zcall APIs, including ID-checked tool results.

## Drive a loop without surrendering control

```rust
use zharness::{ToolLoop, hooks::NoHooks, turn::{Action, CallSlot, LoopLimits}};

let mut slots = [const { CallSlot::<1024>::new() }; 4];
let mut flow = ToolLoop::new(7, &mut slots, LoopLimits::default());
let mut hooks = NoHooks;
assert_eq!(flow.next(&mut hooks)?, Action::CallModel { run: 7, round: 1 });
// The application has not sent a model request yet. Feed real zcall events into
// flow.observe(...), then execute only the tickets returned by flow.next(...).
assert_eq!(flow.next(&mut hooks)?, Action::Pending);
# Ok::<(), zharness::Error>(())
```

The controller accepts real zcall `ToolReady` events and waits for a successful
`WaitingTools` terminal observation before scheduling tools. Multiple tickets may
be issued and completed in any order. Retain outputs/history, acknowledge the
zcall Session and call `commit_result`; only then can another model action be
issued. This is a state machine, not an automatic network loop.

Hooks compose as `(audit, policy)`. `Allow`, `Pause` and `Deny` gate proposed
actions before state changes. After observers cannot veto an already committed
operation. A paused hook may run again and must not perform the proposed action.

## Run the source examples

```sh
cargo test --locked --all-targets
cargo test --locked --doc
python scripts/examples.py
cargo run --locked --example context_window
cargo run --locked --example tool_loop
cargo run --locked --example hooks
cargo run --locked --example agent_cluster
cargo bench --locked --bench dispatch
```

`tool_loop` encodes actual zcall requests, consumes interleaved fixture bytes in
17-byte chunks, retains two real call IDs, completes tools in reverse order and
builds the next model request from retained history. `agent_cluster` demonstrates
two independent tasks, a dependency join and mailbox delivery. These examples
run without network credentials; they do not claim to launch remote agents.

## Contracts

All proportional library storage comes from the caller. Capacity failures never
silently expand buffers or replay external side effects. `memory_bytes` describes
owned state and supplied storage, not borrowed payloads, process RSS or total
thread stacks. Core operations allocate no heap memory; application tools/hooks
and generic mailbox payload types can still allocate on their own.

JSON syntax and capability bits are not permission grants or complete JSON Schema
validation. Applications own business rules, authorization and safe retries.
Agent leases reject stale board completions; they do not undo external work or
provide cross-machine exactly-once execution. Give each run/board a fresh identity.

See [architecture](ARCHITECTURE.md), [context](CONTEXT.md), [loop](HARNESS.md),
[hooks](HOOKS.md), [agents](AGENTS.md), [tools](TOOLS.md),
[bridge](ZCALL_BRIDGE.md), [security](SECURITY.md), and [performance](PERFORMANCE.md).
