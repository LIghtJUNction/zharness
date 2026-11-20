//! Native zguard authorization immediately before tool and command execution.
//!
//! Each Authorization holds trusted host state for one attempt. Rejections and
//! failed audit writes leave output buffers untouched. Reuse Guard across the
//! session to retain its budgets. The library never reads approval or capability
//! fields from model arguments. OS isolation and adapter behavior remain host-owned.
use crate::{
    Error,
    bridge::{Completion, Prepared},
    command::{Command, CommandError, CommandExecutor, CommandOutput, CommandResult},
    hooks::{HookResult, Hooks, NoHooks},
    tools::{Executor, ToolOutput},
    validation::Arguments,
};
use zguard::{Approval, Audit, Guard, Request, Verdict};

/// One attempt's guard, audit sink, monotonic host time and optional approval.
///
/// Construct a fresh value at each execution boundary. Approval is moved into
/// the next check, including a failed check; it cannot be replayed.
#[must_use]
pub struct Authorization<'g, 'p, 'r, A: Audit> {
    guard: &'g mut Guard<'p>,
    audit: &'g mut A,
    now: u64,
    approval: Option<Approval<'p, 'r>>,
}
impl<'g, 'p, 'r, A: Audit> Authorization<'g, 'p, 'r, A> {
    pub fn new(guard: &'g mut Guard<'p>, audit: &'g mut A, now: u64) -> Self {
        Self {
            guard,
            audit,
            now,
            approval: None,
        }
    }
    /// Attach a request-bound approval issued through the trusted host channel.
    pub fn with_approval(mut self, approval: Approval<'p, 'r>) -> Self {
        self.approval = Some(approval);
        self
    }
    fn authorize(self, request: Request<'_>) -> Verdict {
        self.guard
            .authorize(request, self.now, self.approval, self.audit)
    }
}

/// Protect an existing Executor, including a plugin Router or an MCP adapter.
///
/// Prepared::execute_guarded supplies the real call ID automatically. Direct
/// users supply its exact ID and create a fresh wrapper for each attempted call.
/// Each wrapper accepts exactly one execute attempt; reuse returns InvalidState.
/// Calling a separately held unguarded adapter remains a host-level bypass.
pub struct GuardedExecutor<'e, 'g, 'p, 'r, E: Executor + ?Sized, A: Audit> {
    executor: &'e mut E,
    authorization: Option<Authorization<'g, 'p, 'r, A>>,
    call_id: &'r str,
    last_verdict: Option<Verdict>,
}
impl<'e, 'g, 'p, 'r, E: Executor + ?Sized, A: Audit> GuardedExecutor<'e, 'g, 'p, 'r, E, A> {
    pub fn new(
        executor: &'e mut E,
        call_id: &'r str,
        authorization: Authorization<'g, 'p, 'r, A>,
    ) -> Self {
        Self {
            executor,
            authorization: Some(authorization),
            call_id,
            last_verdict: None,
        }
    }
    pub fn last_verdict(&self) -> Option<Verdict> {
        self.last_verdict
    }
}
impl<E: Executor + ?Sized, A: Audit> Executor for GuardedExecutor<'_, '_, '_, '_, E, A> {
    fn execute(
        &mut self,
        name: &str,
        arguments: Arguments<'_>,
        output: &mut [u8],
    ) -> Result<ToolOutput, Error> {
        let authorization = self.authorization.take().ok_or(Error::InvalidState)?;
        let verdict =
            authorization.authorize(Request::tool(self.call_id, name, arguments.as_bytes()));
        self.last_verdict = Some(verdict);
        if !verdict.is_allowed() {
            return Err(Error::Guard(verdict));
        }
        self.executor.execute(name, arguments, output)
    }
}

/// Protect a CommandExecutor using the descriptor's exact structure.
/// Each wrapper accepts one attempt, including an invalid command; reuse fails.
///
/// Program resolution, environment, OS access and process lifetime belong to the
/// adapter. Allowing a program grants its full capabilities unless the host
/// applies further argument and resource policy.
pub struct GuardedCommandExecutor<'e, 'g, 'p, 'r, E: CommandExecutor + ?Sized, A: Audit> {
    executor: &'e mut E,
    authorization: Option<Authorization<'g, 'p, 'r, A>>,
    last_verdict: Option<Verdict>,
}
impl<'e, 'g, 'p, 'r, E: CommandExecutor + ?Sized, A: Audit>
    GuardedCommandExecutor<'e, 'g, 'p, 'r, E, A>
{
    pub fn new(executor: &'e mut E, authorization: Authorization<'g, 'p, 'r, A>) -> Self {
        Self {
            executor,
            authorization: Some(authorization),
            last_verdict: None,
        }
    }
    pub fn last_verdict(&self) -> Option<Verdict> {
        self.last_verdict
    }
}
impl<E: CommandExecutor + ?Sized, A: Audit> CommandExecutor
    for GuardedCommandExecutor<'_, '_, '_, '_, E, A>
{
    fn execute(
        &mut self,
        command: Command<'_>,
        stdout: &mut [u8],
        stderr: &mut [u8],
    ) -> Result<CommandOutput, CommandError> {
        let authorization = self
            .authorization
            .take()
            .ok_or(CommandError::InvalidState)?;
        command.validate()?;
        let verdict = authorization.authorize(Request::command(
            command.id,
            command.program,
            command.args,
            command.cwd,
        ));
        self.last_verdict = Some(verdict);
        if !verdict.is_allowed() {
            return Err(CommandError::Guard(verdict));
        }
        self.executor.execute(command, stdout, stderr)
    }
}

impl<'a> Prepared<'a> {
    /// Authorize the exact call, audit it, then invoke the executor once.
    /// Deny and RequireApproval both return Error::Guard before the adapter runs.
    /// An adapter failure still consumes the authorized attempt's budget/approval.
    pub fn execute_guarded<'o, E: Executor + ?Sized, A: Audit>(
        self,
        executor: &mut E,
        output: &'o mut [u8],
        authorization: Authorization<'_, '_, 'a, A>,
    ) -> Result<Completion<'a, 'o>, Error> {
        let mut guarded = GuardedExecutor::new(executor, self.call_id(), authorization);
        self.execute(&mut guarded, output)
    }
    /// Run hooks, then authorize/audit at the actual executor boundary.
    /// Hook pause/deny never reaches the guard. Guard rejection reaches the
    /// existing execution-error observer without invoking the underlying adapter.
    pub fn execute_guarded_with_hooks<'o, E: Executor + ?Sized, A: Audit>(
        self,
        executor: &mut E,
        output: &'o mut [u8],
        authorization: Authorization<'_, '_, 'a, A>,
        hooks: &mut impl Hooks,
    ) -> Result<HookResult<Completion<'a, 'o>>, Error> {
        let mut guarded = GuardedExecutor::new(executor, self.call_id(), authorization);
        self.execute_with_hooks(&mut guarded, output, hooks)
    }
}
impl<'a> Command<'a> {
    /// Validate, authorize/audit this exact descriptor, then invoke the adapter once.
    pub fn execute_guarded<'o, E: CommandExecutor + ?Sized, A: Audit>(
        self,
        executor: &mut E,
        stdout: &'o mut [u8],
        stderr: &'o mut [u8],
        authorization: Authorization<'_, '_, 'a, A>,
    ) -> Result<CommandResult<'a, 'o>, CommandError> {
        match self.execute_guarded_with_hooks(
            executor,
            stdout,
            stderr,
            authorization,
            &mut NoHooks,
        )? {
            HookResult::Applied(result) => Ok(result),
            // NoHooks never pauses. Stay closed if that contract changes.
            HookResult::Paused => Err(CommandError::Denied),
        }
    }
    /// Run hooks before the shared guarded CommandExecutor boundary.
    pub fn execute_guarded_with_hooks<'o, E: CommandExecutor + ?Sized, A: Audit>(
        self,
        executor: &mut E,
        stdout: &'o mut [u8],
        stderr: &'o mut [u8],
        authorization: Authorization<'_, '_, 'a, A>,
        hooks: &mut impl Hooks,
    ) -> Result<HookResult<CommandResult<'a, 'o>>, CommandError> {
        let mut guarded = GuardedCommandExecutor::new(executor, authorization);
        self.execute_with_hooks(&mut guarded, stdout, stderr, hooks)
    }
}
