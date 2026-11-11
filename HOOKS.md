# Hook systems and boundaries

zharness has two complementary interfaces. Existing `Hooks` callbacks remain
source-compatible and directly cover native tool-loop scheduling, context changes,
agent assignments, actual tool/command execution and output processing. The new
unreleased `lifecycle` module supplies typed, independent channels for wider
conversation, configuration, transport, reply and application-storage boundaries.
No global callback registry or runtime is added by either interface.

## Existing native callbacks (unchanged)

| Boundary | Gate | Completion | Attempt failure |
|---|---|---|---|
| Model/tool scheduling and context/agent state | before(Before) | after(After) | Returned error |
| Actual validated Executor | before_execute | after_execute | execute_error |
| Command adapter | before_command | after_command | command_error |
| Result reduction | before_result | after_result | Returned transform error; no after_result |

Before::Tool issues a ticket; it is not actual execution. After::ToolFinished is
loop-result commitment; it is not just an Executor return. Paused/denied execution
never invokes the adapter or modifies output. Business-error tool results and
nonzero command exits remain completed unsuccessful results, distinct from an
adapter error. Output reduction remains an explicit step before history commitment.
Existing tuple/reference forwarding and default callback methods are unchanged.

## Grouped lifecycle channels (new source API)

Use `Gate<E>`, `Observer<E>` and `Interceptor<E>` independently for typed event E.
The ten families are conversation, model, prompt, request, response, tool, context,
agent, persistence and runtime. They cover 97 action labels without expanding the
legacy Hooks trait. Gate controls proposals; Observer records facts; Interceptor
edits a copied candidate before validation/gating. See LIFECYCLE.md for every action,
exact native entry points, compiled examples and manual-integration boundaries.

`Lifecycle::run` surrounds a real caller operation. `begin`/`finish` lets an app
schedule/await work itself and later finish the correlated receipt. `notify` is
only for already-observed facts. None of these methods creates a UI, model call,
checkpoint store, rate limiter or remote worker by declaring an event name.

Native entry points include Conversation message acceptance/edit/delete/start/
close, ModelSettings model/reasoning/route/options changes, a complete-message
Sender wrapper, Context request/budget/fork/compaction wrappers and ResponseStream
for real zcall events. Final assistant acceptance is distinct from a model turn's
WaitingTools terminal outcome. Legacy and grouped channels are not automatically
duplicated; applications choose which explicit boundaries to compose.

## Shared operational rules

Gates short-circuit left-to-right with Allow, Pause or Deny. Before callbacks can
repeat and must not perform the proposed action. Observers cannot veto already
committed effects or turn a failure into success. Processing errors do not grant
permission to replay side-effecting tools. Panics/crashes are outside normal-return
observation guarantees, and external I/O may have partial effects before an error.

Use separate state/buffers for concurrent operations or caller synchronization.
Give each Lifecycle coordinator a unique epoch and keep event borrows alive while
its receipts are outstanding. New observers default to payload metadata only;
Visibility::Content opts in to content access. This does not change the original
raw-data contract of legacy command/tool Hooks. Never log secrets indiscriminately.

Direct unhooked Context/Prepared operations, external OS calls and arbitrary UI
changes remain explicit opt-outs. Hooks are policy boundaries, not authentication,
process isolation or universal interception. Adapters own their actual network,
OS, storage, cancellation and environment guarantees.
