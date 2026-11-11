# Typed Harness Lifecycle API

Version 0.2.3 publishes the typed lifecycle APIs that were developed after
v0.2.2. Use the published zharness 0.2.3 crate or its matching source tag.
No integration with an MCP repository is needed; no runtime, database or
transport dependency has been added.

## Three independent primitives

`Gate<E>` decides whether a typed proposal is allowed, paused or denied.
`Observer<E>` records started, completed, failed, deferred or externally observed
facts. `Interceptor<E>` edits a copied candidate before validation and gating.
The callback interfaces are generic over a small domain event, not a giant trait
with a method per every conceivable lifecycle location.

A `Lifecycle<G, O>` owns callback state and monotonically numbered proposals.
There is no global registry, hidden thread, lock or heap-backed event bus. Tuples
compose gates/interceptors/observers in order. Gates stop at the first non-Allow;
interceptors stop on error; all normal-return observers run in order. NoGate,
NoObserver and NoInterceptor opt out explicitly. The old Hooks trait and its
methods/Before/After variants remain unchanged and continue to work independently.

## Coverage: ten typed families

Each action has the same before/started/finished-or-failed contract when used with
`run` or `begin`/`finish`. These are 97 action labels, not 97 automatically installed
callbacks. `notify` reports a fact that has already happened; it cannot veto it.

| Family | Actions |
|---|---|
| conversation | Start, Close, AcceptUser, AppendMessage, EditMessage, DeleteMessage, Fork, Restore |
| model | Select, Switch, ReasoningChange, OptionsChange, RouteSelect, Fallback, Unavailable |
| prompt | Build, SystemInstructions, InjectContext, ToolDefinitions, Compact, BudgetExceeded |
| request | Build, Encode, Send, Retry, Receive, Headers, Timeout, Cancel |
| response | TurnStart, IdentityUpdated, BlockStart, TextDelta, ReasoningDelta, ToolArgumentsDelta, RefusalDelta, Signature, BlockEnd, ToolReady, Usage, ChoiceEnd, Snapshot, BlockIncomplete, Loss, TurnEnd, FinalizeAssistant, CommitMessage, Cancel |
| tool | Register, Unregister, ParseArguments, Validate, Authorize, Dispatch, Execute, TransformResult, CommitResult, Cancel |
| context | Load, Save, Append, Merge, Fork, Evict, Compact, BudgetChange, BudgetWarning, BudgetExceeded |
| agent | Start, Stop, Assign, Handoff, Join, Complete, Fail, Timeout, Recover, LeaseExpire, SendMessage, ReceiveMessage |
| persistence | SaveConversation, LoadConversation, RestoreHistory, Checkpoint, Snapshot, DeleteSnapshot |
| runtime | BudgetConsume, BudgetExceeded, RateLimit, RateLimitExceeded, Retry, Cancel, Timeout, Error, Interrupt, Resume, Shutdown |

The enum variants are non-exhaustive to allow later action additions. Amount and
revision fields are caller metadata, with units defined for each application
operation. Correlation records conversation/run/request/message/agent IDs and the
application retry attempt separately from the library proposal serial.

## Native integration versus explicit wrapping

Native wrappers already connect the important boundaries to real operations:

| Entry point | What it actually gates/observes |
|---|---|
| Conversation::start/close | The supplied create/close operation and local state transition |
| Conversation::user_message, append_message, edit_message, delete_message | Actual supplied history/storage operation; revision advances only on success |
| Conversation::user_message_intercepted | Final filtered payload acceptance; identity/action/revision cannot be rewritten by the filter |
| ModelSettings::select/switch_model/set_reasoning/set_route/set_options/configure | Committed local configuration, with actual previous and proposed values |
| Context::build_with_lifecycle | Real zcall request building, including returned capacity/ID errors |
| Context::set_budget_with_lifecycle, fork_with_lifecycle, compact_with_lifecycle | Native checked context operations |
| request::send_with_hooks | One actual Sender invocation, with complete-message length verification |
| ResponseStream::accept/finalize/cancel | Acceptance of genuine zcall events, final assistant commit and logical cancellation |

Prompt assembly, retry policy, checkpoint databases, remote workers, limits and
other application-owned boundaries use `Lifecycle::run` around the actual
operation. Declaring an action or calling `notify` alone does not implement those
systems. Existing tool/command/output hooks remain native to their established
entry points; they are not silently duplicated into the new channels.

## Model and reasoning settings

```rust
use zharness::hooks::HookResult;
use zharness::lifecycle::{Correlation, Fault, Lifecycle, NoGate, NoObserver};
use zharness::lifecycle::domains::model::{Configuration, Reasoning};
use zharness::settings::ModelSettings;

let mut hooks = Lifecycle::new(100, NoGate, NoObserver);
let HookResult::Applied(mut settings) = ModelSettings::select(
    Correlation::default(), Configuration::new("model-a"), &mut hooks,
)? else { unreachable!("NoGate does not pause"); };
let _ = settings.switch_model("model-b", &mut hooks)?;
let _ = settings.set_reasoning(Reasoning::Level("high"), &mut hooks)?;
assert_eq!(settings.current().model, "model-b");
assert_eq!(settings.revision(), 2);
# Ok::<(), Fault>(())
```

A real model Gate validates allowed models, provider capabilities and budget.
Reasoning::Level preserves application intent without inventing a universal
provider vocabulary. Settings do not modify an already-built Request or an
in-flight Session. The application snapshots current settings and maps them through
zcall's supported options for its next request. Recompute model-specific context
costs after a model change; changing a label cannot guarantee compatibility.

## Bracket a real operation

```rust
use zharness::hooks::HookResult;
use zharness::lifecycle::{Correlation, Fault, Lifecycle, NoGate, NoObserver};
use zharness::lifecycle::domains::persistence;

let mut hooks = Lifecycle::new(101, NoGate, NoObserver);
let mut stored = false;
let result = hooks.run(
    Correlation::default(),
    persistence::Event::new(persistence::Action::Checkpoint),
    |_| { stored = true; Ok(()) },
)?;
assert_eq!(result, HookResult::Applied(()));
assert!(stored);
# Ok::<(), Fault>(())
```

Replace the example's memory mutation with the real storage call. The closure
runs at most once per `run` invocation, only after an allowed valid proposal.
Errors are observed and returned without replacement or automatic retry. A failed
operation may already have changed external state; the hook layer is not a
transaction manager. It never claims that a request was saved merely because an
application sent a SaveConversation event.

## Async boundaries and completion pairing

```rust
use zharness::hooks::HookResult;
use zharness::lifecycle::{Correlation, Fault, Lifecycle, NoGate, NoObserver, Outcome};
use zharness::lifecycle::domains::request;

let mut hooks = Lifecycle::new(102, NoGate, NoObserver);
let HookResult::Applied(mut pending) = hooks.begin(
    Correlation::default(), request::Event::new(request::Action::Send),
)? else { unreachable!(); };
// The application performs/awaits its actual operation outside the hook layer.
// This example reports abandonment without pretending a network send happened.
hooks.finish(&mut pending, Outcome::Abandoned)?;
assert!(pending.is_finished());
assert!(hooks.finish(&mut pending, Outcome::Succeeded).is_err());
# Ok::<(), Fault>(())
```

Span is private-field, non-Clone and must-use. Several receipts can be outstanding
and finish out of order. Duplicate completion and different-epoch dispatch are
rejected without emitting another terminal notice. A rejected finish preserves
the receipt so routing can be corrected. Dropping a pending receipt does not
cancel external work or manufacture an observation: explicitly finish, fail,
cancel, defer or abandon it.

Give every Lifecycle coordinator instance a distinct epoch, including after
restart/recreation. Reusing the same epoch across instances defeats cross-instance
receipt detection. Epochs/IDs are application correlation, not security tokens.
There is no global allocator of identities. Keep borrowed event data alive until
finish, and serialize access to each coordinator. Panics/crashes are not caught;
normal-return pairing is not an exactly-once external execution guarantee.

## Sending, streaming and final replies are different boundaries

Sender acceptance means exactly what the supplied adapter documents: for example,
queued locally or fully written. A send completion does not prove provider receipt,
model generation, response delivery or persistence. A partial-length receipt is
an InvalidOutput failure, even though bytes may already have left the process.
The wrapper never retries on its own; the application owns idempotency/recovery.

ResponseStream maps only public zcall EventData, not provider-specific JSON.
TurnStart means a real normalized response start. first_visible identifies the
first accepted nonempty text/refusal delta, separately from reasoning or tool
arguments. ReasoningDelta exposes only what the provider returned; it does not
reveal hidden internal reasoning. The application consumer still receives the
original EventData, preserving metadata not repeated in the hook view.

TurnEnd retains the exact TurnOutcome. Successfully accepting a failed terminal
event means the observation was consumed, not that the model call succeeded.
WaitingTools is not finalization. finalize runs a separate caller operation only
after TurnOutcome::Completed; it prevents duplicate finalization and rejects
waiting-tools, failed, incomplete or cancelled turns. It does not infer that the
whole agent task is done or persist/render text without a caller operation.

A gate Pause does not invoke the stream consumer. A consumer Pause produces a
Deferred observation and leaves accepted counters/state unchanged. The same event
must be replayed through zcall with only the unconsumed byte suffix. The consumer
must not commit side effects then return Pause. Errors, including Deny, return to
the driver; it must explicitly stop/cancel or safely resume the Session. Logical
ResponseStream cancellation does not stop network reads by itself.

## Privacy and interception

Payload Debug prints only byte counts and whether content was supplied. New
observers receive redacted payloads by default: body pointers are removed, not
merely hidden in formatting. Visibility::Content explicitly permits access to
supplied content. Gates can inspect supplied content when making policy decisions.
Model names, routing labels, numeric amounts and IDs remain metadata; do not put
secrets in those fields. Custom Event implementations must correctly implement
redacted. Legacy Hooks keep their original raw-data contracts; this default does
not retroactively change command/tool callbacks.

Interceptors edit copied candidates; original immutable data is unchanged.
The final candidate is validated and gated before it reaches the operation.
Failure never falls back to raw input. Native user-message/configuration filters
can change only content/proposed settings, not correlation, prior settings or
revision. Generic custom event transforms are trusted application code and must
preserve their declared semantics. Text reduction still uses the separate
caller-buffered ToolResultTransform primitives where appropriate.

## Reproduction

`cargo run --locked --example conversation_hooks` executes a complete local flow:
message approval and history acceptance, settings changes, actual zcall request
construction/encoding, memory-send acceptance, a captured model response parsed
by zcall, assistant finalization and a memory persistence operation. It makes no
live provider request and requires no credentials. Model-reasoning intent is
changed and reset explicitly rather than falsely claiming provider mapping.

The lifecycle, lifecycle_adapters, lifecycle_policy and lifecycle_replay tests
cover domains, pairing, ordering, privacy, final-candidate checks, send failures,
state mutation, real stream replay and finalization boundaries. Every legacy test
remains in CI. `cargo bench --locked --bench lifecycle` measures 13 local cases,
including no-hook/paired-hook dispatch, split finish, settings changes, message
acceptance, memory sending and normalized-event handling. It does not measure
network/model/database latency, tokenizer accuracy or prove zero runtime cost.
