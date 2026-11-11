# Model/tool loop primitives

ToolLoop is an application-driven state machine. Calling next issues one action;
it does not call a model, run a tool, sleep or create a task. Drive it from a sync
loop, a future, an event loop or a distributed coordinator chosen by the caller.

```text
Ready -> CallModel -> Receiving
                       | real zcall events
                       +-- terminal Completed -> Stopped
                       +-- terminal failure   -> Stopped
                       +-- terminal WaitingTools -> Tools
                                                    |
                           Execute tickets (possibly several concurrently)
                                                    |
                            retained results + commit_result for each ticket
                                                    |
                           next -> CallModel, or Stopped at the round limit
```

## Observation and dispatch

Create a distinct application run ID and caller-owned CallSlot<N> array. N is
bytes per retained call: the real call ID, name and complete argument object
share that storage. LoopLimits bounds model rounds and total tool calls. The
controller retains genuine zcall EventData::ToolReady; it never fabricates a
provider event or executes partial argument deltas.

Pass events from Session::step to observe. Return Control::Continue only after
observe succeeds. An event that cannot fit accepts no partial invocation. For
recoverable slot-count exhaustion, return Pause, replace_slots with larger caller
storage and resume the same Session with only its unconsumed input suffix. The
recovery integration test exercises this actual replay path. Per-call byte size
N is a fixed configuration; a limit/policy failure should abort or cancel unless
the application has a safe recovery strategy.

ToolReady alone does not authorize execution. The controller waits for zcall's
WaitingTools terminal outcome. next then issues Execute tickets after the
Before::Tool hook allows them. Issue several tickets to run concurrently, or
finish one before asking for another. Prepared arguments and authorization remain
mandatory at the application's execution boundary.

## Completion and persistence

An invocation returned by invocation(ticket) borrows controller storage. Copy it
into application transcript storage when it must outlive the next mutable
controller operation. The example keeps history InvocationStorage buffers and
result arrays separate from per-turn CallSlot storage.

Retain the tool result, accept it into application history, acknowledge it through
Completion::submit to the zcall Session, and call ToolLoop::commit_result. These
are explicit boundaries, not an atomic transaction across memory, external I/O
and persistent storage. commit_result checks the original call ID and rejects
stale or duplicate completion tickets. It does not perform the history/session
operations implicitly. Failures use a retained Completion with is_error=true.

Once all results are committed, next can issue the following CallModel action.
It clears per-turn retention only after the before-model hook allows that action.
Durable history must already exist elsewhere. Context::build constructs the next
Request from that history via zcall, without loss of real tool-call IDs.

## Pause, stop and cancellation

Pending means model input or dispatched work is still outstanding. Paused means
a before hook deferred a proposal without dispatching it. Deny is Error::Denied
and leaves the proposal uncommitted; the application chooses whether to cancel,
revise policy or produce an explicit error result. No implicit retry occurs.

cancel marks the controller stopped and rejects late commits. It cannot stop an
already-running process, revoke a network request or undo a side effect. Model
failure, explicit cancellation and round-limit exhaustion are distinct stop
reasons. Use fresh run IDs after recreation; tickets are not authentication tokens.

`examples/tool_loop.rs` composes Context, Hooks, a real zcall Session and the tool
executor for two recorded model rounds. `tests/turn.rs`, `tests/recovery.rs` and
`tests/native_example.rs` execute normal, denied, incomplete, cancelled, replay
and out-of-order paths without live provider credentials.
