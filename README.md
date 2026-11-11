# zharness

Caller-buffered primitives for developer-owned AI harnesses: context windows,
model/tool turns, hooks, output transforms and multi-agent coordination. The core
is `no_std`, forbids handwritten unsafe Rust and directly depends on
`zcall = "=0.1.2"` and `zplugin = "=0.1.0"` with default features disabled.
zplugin's official macro generates the native interface and introduces
compilation-only parser dependencies; the runtime dependency graph stays within
Project Z crates.

Version 0.2.4 adds native skills and zplugin extensions alongside the typed
lifecycle channels and conversation, model-settings, request-send and response
adapters introduced in 0.2.3. Existing Hooks implementations and lifecycle enum
variants remain source-compatible. The library itself does not start an HTTP
client or agent runtime.

To use the published package:

```toml
[dependencies]
zharness = "=0.2.4"
```

Default features enable filesystem skill discovery and native path loading.
Set `default-features = false` to use the `no_std` core.

## Boundary

The library maintains bounded state and returns actions. Your application owns
I/O, model selection, authorization, scheduling and persistence. No HTTP client,
background worker, retry engine, tokenizer, summarizing model or database is
started implicitly. MCP is developed independently in
[zmcpio](https://github.com/LIghtJUNction/zmcpio); this repository contains no MCP
integration directory or cross-repository build dependency.

| Module | Atomic capability |
|---|---|
| `context` | Grouped history, budgets, pinned retention, eviction, explicit summaries, forks and zcall request construction |
| `turn` | Model/tool actions, real ToolReady retention, bounded rounds/calls and out-of-order completion |
| `hooks` | Existing native before/completion/error callbacks for tools, commands, results and coordination |
| `lifecycle` | New independent typed Gate/Observer/Interceptor channels, correlation and paired operations |
| `conversation`, `settings` | New gated message acceptance and committed model/reasoning/configuration changes |
| `request`, `response` | New actual-send boundary and replay-aware real zcall event consumption/final reply acceptance |
| `execution`, `command` | Hooked user executors, preserved IDs/status and bounded output checks; no OS runtime |
| `output` | Caller-buffered text reduction and validated JSON minification |
| `cluster` | Capability matching, task dependencies, leases, handoff, joins and bounded mailboxes |
| `skills` | Borrowed SKILL.md metadata, caller-buffered summaries, ordered directories and on-demand loading |
| `plugin` | zplugin native loader, versioned byte-call interface, checked outputs and an Executor bridge |
| `tools`, `validation`, `bridge` | Tool definitions, argument policy and public zcall result APIs |

## Native extensions

`plugin::Plugin` exposes zplugin's actual native loader. Load an explicitly chosen
trusted library, create `ExtensionApi`'s typed proxy, then call operations with
borrowed input and caller-owned output buffers. The same proxy implements
`Executor`, so it works with the existing tool registry and execution hooks.

The v1 extension interface has one operation: `call(operation, input, output)`.
The library checks returned lengths and can validate UTF-8; the application owns
operation semantics, payload schemas, scheduling, permissions and recovery.
There is no automatic plugin scan, tool registration, execution or reload loop.
See [PLUGINS.md](PLUGINS.md) for loading, plugin authoring, lifecycle and examples.

## Native skills

`SkillPaths::from_env(project)` combines project and global skills with any
directories in `SKILLS_PATH`. Earlier valid names win, while unrelated global
skills remain available. The environment follows the host PATH syntax and
precedes the defaults. Use `prepend` or `push` for software-specific directories,
or `SkillPaths::new(roots)` for an exact search plan.

```sh
export SKILLS_PATH="$PWD/.agents/skills:$HOME/.claude/skills:/opt/editor/skills"
cargo run --locked --example skills
```

Discovery retains each skill's name, description and location. Summary generation
writes escaped XML into your buffer; selected bodies and resources load on demand.
The default `std` feature enables the filesystem adapter. Disabling default
features retains the borrowed parser and summary writer with no `alloc` or I/O.
See [SKILLS.md](SKILLS.md) for Rust examples, precedence, diagnostics and limits.

## Grouped lifecycle hooks (0.2.3)

Use a small typed event rather than a hundred methods in one Hooks trait. The
families are conversation, model, prompt, request, response, tool, context, agent,
persistence and runtime. Gates decide; interceptors change copied candidates;
observers record facts. Observers get payload metadata by default, with explicit
content opt-in. Tuples compose statically; there is no global mutable registry.

`Lifecycle::run` wraps an actual operation. `begin`/`finish` supports application-
owned async work and out-of-order completion without an async runtime. Native
adapters already cover user acceptance, message edits/deletes, model and reasoning
changes, request building/sending, stream consumption and assistant finalization.
Storage, routing, rate limits and other application-owned work use explicit wrappers,
not fictitious automatic interception. See [LIFECYCLE.md](LIFECYCLE.md) for the full
97-action matrix, compiled examples, privacy and failure/replay contracts.

An accepted send is not provider receipt. A model turn ending in WaitingTools is
not a final assistant reply. A paused stream event must be replayed before it is
counted as accepted. ModelSettings records intent for future requests; provider
capability checks and wire mappings still belong to the application and zcall.

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
// Caller estimates for illustration; use the target model's tokenizer in production.
context.push(Group::new(0, &system, 16).pinned(), Overflow::Reject)?;
context.push(Group::new(1, &user, 20), Overflow::EvictOldest)?;
assert_eq!(context.tokens(), 36);
# Ok::<(), zharness::Error>(())
```

Keep related calls/results in one Group. Eviction/compaction never splits a group.
Forks copy descriptors, not borrowed content; explicit summaries come from the
application. Context::build delegates protocol representation and checked tool
results to zcall instead of creating another provider adapter.

## Drive a loop without surrendering control

```rust
use zharness::{ToolLoop, hooks::NoHooks, turn::{Action, CallSlot, LoopLimits}};

let mut slots = [const { CallSlot::<1024>::new() }; 4];
let mut flow = ToolLoop::new(7, &mut slots, LoopLimits::default());
let mut hooks = NoHooks;
assert_eq!(flow.next(&mut hooks)?, Action::CallModel { run: 7, round: 1 });
// The application has not sent a model request yet.
assert_eq!(flow.next(&mut hooks)?, Action::Pending);
# Ok::<(), zharness::Error>(())
```

Feed genuine zcall events to observe, then execute only approved Execute tickets.
ToolReady is retained until WaitingTools is confirmed. Several tickets can finish
out of order. Retain history/results, acknowledge the Session and commit each
result explicitly before another model action is issued. No hidden loop performs
network requests or retries side effects.

## Execution and output processing

Prepared::execute_with_hooks and Command::execute_with_hooks gate actual user
executors, distinct from scheduling or history commitment. Pause/deny does not
invoke an adapter. Nonzero command exits and business tool errors retain their
status; invalid output or adapter failures remain errors. The core does not spawn
processes, interpret shell strings or enforce adapter timeouts.

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

Reduction preserves identity/status. Retry failed output processing with retained
raw input, not by rerunning the tool. JsonMinify validates through zcall; text
truncation is explicitly lossy and does not promise valid truncated JSON or exact
token savings. Custom semantic summaries remain an application decision.

## Run from the source checkout

```sh
cargo test --locked --all-targets
cargo test --locked --doc
python scripts/examples.py
cargo run --locked --example conversation_hooks
cargo run --locked --example tool_loop
cargo run --locked --example command_hooks
cargo bench --locked --benches
```

The conversation example covers user approval, settings changes, real zcall
request encoding, an in-memory send, recorded response parsing and explicit final
persistence. Tool-loop tests cover real interleaved calls. Command hooks run
controlled child processes on the host. No live provider credentials are required;
these examples do not claim real network/model/database integration.

All proportional core storage is caller-owned. Generic callback code and adapters
can independently allocate or cause side effects. Hooks are not a sandbox, IDs
are not authentication, and leases do not provide external exactly-once execution.
Read [architecture](ARCHITECTURE.md), [typed lifecycle](LIFECYCLE.md),
[hook boundaries](HOOKS.md), [context](CONTEXT.md), [loop](HARNESS.md),
[commands](COMMANDS.md), [output](OUTPUT.md), [agents](AGENTS.md),
[skills](SKILLS.md), [plugins](PLUGINS.md), [security](SECURITY.md), and
[performance](PERFORMANCE.md).
