# Lifecycle hooks

Hooks are explicit, caller-owned control points, not a global interception
mechanism. Use `(audit, policy)` or nested tuples to compose statically dispatched
callbacks without mandatory allocation. Gates short-circuit left-to-right;
normal-return observers all run in the same order and cannot veto results.

## Select the boundary

| Boundary | Before gate | Completion observation | Attempt failure |
|---|---|---|---|
| Model/tool scheduling and context/agent state | before(Before) | after(After) | Caller handles the returned error |
| Actual validated Executor call | before_execute | after_execute | execute_error |
| Command adapter invocation | before_command | after_command | command_error |
| Optional result reduction | before_result | after_result | Caller handles transform error; no after_result |

Before::Tool means a ToolLoop ticket is about to be issued, not that a process
has started. After::ToolFinished means the application committed the loop result,
not that an Executor has just returned. The added execution callbacks close that
gap without changing the meanings or variants of existing lifecycle enums.

## Gate and observer rules

Allow permits the operation. Pause returns HookResult::Paused (or Action::Paused
at the ToolLoop layer) without performing the proposed action. Deny returns the
corresponding Denied error. In the execution wrappers, paused/denied calls leave
output buffers unchanged, never invoke the adapter and produce no completion or
attempt-failure callback. A before callback may therefore be evaluated again;
it must not perform the action or assume it is observing a once-only attempt.

After callbacks see committed state or validated output for their documented
boundary. They cannot roll back side effects, replace return values or initiate
implicit retries. Execution error observers receive the original error and cannot
suppress it. Panics are not caught by the no_std library: guarantees about a final
callback apply to normal Rust returns, not crashes or unwinding hook code.

Tool business failures (ToolOutput::failure) are valid completed results, retaining
is_error=true in after_execute. Command nonzero exit codes and signal-like
termination are likewise observed through after_command with success()==false.
Spawn/I/O/timeout/cancellation reports and invalid adapter output are distinct
command_error paths. No helper creates a timeout timer or kills a process itself.

## Execution and processing

Prepared::execute_with_hooks gates immediately before calling the supplied
Executor, then checks output bounds/UTF-8 before after_execute. Command's hooked
entry point checks capture lengths but preserves stdout/stderr as arbitrary bytes.
Applications may implement a command-backed Executor by composing both layers.
Direct Prepared::execute, custom OS calls or unhooked Context operations remain
explicit opt-outs; the library is not a sandbox and does not intercept them.

Completion::transform_with_hooks still owns the optional output-processing stage.
Its gate receives read-only identity/status metadata and original text; after_result
sees validated processed text and original length. Identity and failure flags are
preserved. A processing retry must use retained input and must not re-execute the
tool. See OUTPUT.md for Chain, JSON minification and semantic-safety rules.

Each added method has a default implementation. Existing Hooks implementations,
Before/After enum matches, tuple chains and mutable-reference forwarding remain
compatible. Use separate callback state for parallel operations or synchronize it
in the application. No hidden lock, global registry or spawned worker is added.

## Security and audit

Hook arguments may contain secrets, raw command output or untrusted text. Do not
log them indiscriminately. Capability bits, command IDs, registered tool names and
string allowlists are not authentication or OS isolation. Approve dangerous work
at its actual execution boundary and keep the adapter's environment/sandbox policy
consistent with the decision. NoHooks allows every structurally valid proposal.

For asynchronous approval, return Pause before execution, obtain the decision in
the application and invoke again only when authorized. The synchronous wrappers
do not host an async runtime. Long-running work, cancellation and external effects
remain application responsibilities; do not claim exactly-once execution from a
callback counter. Read COMMANDS.md and run the real command_hooks example.
