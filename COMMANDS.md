# Command execution hooks

Command hooks surround an explicitly supplied command adapter, not every process
in the host application. The core remains no_std and does not spawn a process,
interpret a shell string, create threads, wait on I/O or allocate capture buffers.
Use the hooked entry point to obtain its guarantees; direct OS calls bypass it.

## Minimal complete example

This memory adapter demonstrates the contract without starting an OS process.
The actual process adapter is in `examples/command_hooks.rs`.

```rust
use zharness::command::{Command, CommandError, CommandExecutor, CommandOutput, ExitStatus};
use zharness::hooks::{Decision, HookResult, Hooks};

struct Approval { approved: bool }
impl Hooks for Approval {
    fn before_command(&mut self, command: Command<'_>) -> Decision {
        if command.program != "approved-program" { return Decision::Deny; }
        if self.approved { Decision::Allow } else { Decision::Pause }
    }
}
struct MemoryAdapter { calls: usize }
impl CommandExecutor for MemoryAdapter {
    fn execute(&mut self, _: Command<'_>, _: &mut [u8], _: &mut [u8])
        -> Result<CommandOutput, CommandError>
    {
        self.calls += 1;
        Ok(CommandOutput { status: ExitStatus::Exited(0), stdout_len: 0, stderr_len: 0 })
    }
}
let command = Command::new("command-1", "approved-program", &["--literal-argument"]);
let mut adapter = MemoryAdapter { calls: 0 };
let mut approval = Approval { approved: false };
assert!(matches!(command.execute_with_hooks(&mut adapter, &mut [], &mut [], &mut approval)?, HookResult::Paused));
assert_eq!(adapter.calls, 0);
approval.approved = true;
let HookResult::Applied(result) = command.execute_with_hooks(&mut adapter, &mut [], &mut [], &mut approval)?
    else { unreachable!("approval was supplied"); };
assert!(result.status.success());
assert_eq!(adapter.calls, 1);
# Ok::<(), CommandError>(())
```

## Event contract

`Command::execute_with_hooks` validates nonempty identities/executables and rejects
NULs in executable/arguments/cwd. These are structural checks, not executable
resolution or permission checks. Invalid descriptors never reach the hooks or
adapter. Empty literal arguments are allowed; an explicitly empty cwd is not.

`before_command` sees the exact command ID, program, argument slice and optional
working directory. Tuple chains evaluate left-to-right. Pause returns
HookResult::Paused; Deny returns CommandError::Denied. Both prevent the adapter
call and leave stdout/stderr buffers untouched. No after/error callback describes
these as attempted process executions.

After Allow, the adapter is invoked once. A normal completed result first passes
capture-length checks, then `after_command` receives the original description,
exit status and borrowed stdout/stderr bytes. Nonzero exit codes and termination
without a code remain completed results with success() == false. They are not
silently normalized to success or automatically retried.

Adapter errors call `command_error` and return that same bounded error. Categories
include Spawn, Io, TimedOut, Cancelled, Capacity and adapter-defined codes. Invalid
capture lengths produce InvalidOutput before after_command can observe invalid
slices. Error observers cannot suppress failure or replace it with success.
Output may be binary: UTF-8 checking belongs to an explicit text conversion.

There is one completion OR error observation per attempted adapter call on normal
return. Hook/adapter panics, process crashes and externally abandoned work are not
caught or guaranteed to generate a final callback. A later explicit invocation is
a new attempt, not an idempotent replay protected by command ID.

## Actual tool execution versus commands

`Prepared::execute_with_hooks` brackets the actual generic Executor invocation
using `before_execute`, `after_execute` and `execute_error`. It retains the real
model call ID, validated arguments and failure flag. Business is_error=true
results invoke after_execute; executor errors, oversized lengths and invalid
UTF-8 invoke execute_error. Pause/deny does not execute or modify output.

These boundaries differ from ToolLoop's Before::Tool ticket issuance and
After::ToolFinished result commitment. A command inside a tool can use its own
command hooks. A typical composition is:

```text
ToolLoop ticket gate
  -> before_execute
     -> application Executor
        -> before_command -> application OS adapter -> after_command / command_error
  -> after_execute / execute_error
  -> before_result -> optional output transforms -> after_result
  -> explicit history and Session acceptance -> ToolLoop result commitment
```

No inner event is fabricated: a tool that does not execute commands emits no
command hooks, and no result processing happens unless requested explicitly.
Execution hooks see raw output, so apply appropriate secrecy policy to observers.
Output transforms remain the separate redaction/reduction layer from OUTPUT.md.

## Adapter responsibilities

Pass argv elements literally; never concatenate them into an implicit shell.
An explicit shell program (sh -c, cmd /C, etc.) is still a shell and requires its
own authorization. A string allowlist does not defeat PATH replacement, symlinks,
working-directory races or executable substitution. Resolve and enforce identities
and privileges at the actual OS boundary when those risks matter.

The descriptor intentionally omits OS-specific environment, stdin, file handles,
resource limits and sandbox policy. Configure those in a trusted adapter and make
them available to application policy out of band. Do not treat the descriptor as
a full audit record of unrepresented OS settings. UTF-8 descriptors do not encode
all possible native OS paths; use a platform-specific application boundary for
non-UTF-8 requirements rather than lossy conversion.

The example clears inherited environment, disables stdin, uses an absolute path
to its own controlled child executable, and drains stdout/stderr concurrently into
bounded buffers. It waits/reaps the child even after capture exceeds capacity.
It tests success, exit code 7, and both pipes exceeding capacity without retry.
It deliberately has no timeout implementation; TimedOut/Cancelled are adapter
reports, not library timers or cancellation mechanisms.

An error after execution may follow real side effects. Do not rerun a command
just to enlarge a capture buffer. Retain available results and apply explicit
idempotency or recovery policy. Parallel commands need separate buffers and hook
state or application synchronization. The library does not schedule workers.

Run `cargo run --locked --example command_hooks`. The example uses std and
application threads; those allocations and OS calls are outside the no_std core.
