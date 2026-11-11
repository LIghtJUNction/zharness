# Lifecycle hooks

Implement Hooks and pass the implementation into hooked operations. The library
stores no global hooks. A tuple such as (audit, policy) composes different hook
types without a required Box, dynamic trait object or allocation. Nested tuples
extend the chain. The caller owns callback state and lifetime.

Before events describe context changes, model actions, tool dispatches and agent
assignments. Return Decision::Allow to permit the transition, Pause to defer it,
or Deny to return Error::Denied. A tuple evaluates before hooks left-to-right
and short-circuits at the first non-Allow result. Library state is not mutated
before all participating gates allow the proposal.

After events observe committed context, model outcomes, tool results, stops,
assignments, task completions, expired leases and cancellations. All tuple
observers run in order and return no decision. A completed side effect must not
be represented as a fresh retryable proposal just because an observer failed.
Callbacks are application code: keep them bounded and do not panic across a
boundary where partial application-side changes would be unsafe.

Pause may cause the same before event to be evaluated again. Before callbacks
must not perform the proposed tool/model action. Store a decision or query an
already-available policy result instead. Network-based approval can be handled
by returning Pause, obtaining approval in the application, then calling next or
the proposed operation again. The library has no asynchronous approval runtime.

Hooks are explicit control points, not automatic authorization. NoHooks allows
all proposals. Direct unhooked Context methods deliberately bypass callbacks.
Task capability masks select eligible workers but do not grant permission to
perform real external operations. Keep actual authorization at execution time.

Run `cargo run --locked --example hooks`. Hook ordering and short-circuiting,
no-mutation pause/deny, and single completion observations are exercised by the
context, turn and cluster test suites.
