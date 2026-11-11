//! Hooks around an actual Executor invocation, distinct from ToolLoop scheduling.
//! No execution, history commit, retry or output transformation occurs implicitly.
use crate::{
    Error,
    bridge::{Completion, Prepared},
    hooks::{Decision, HookResult, Hooks},
    tools::Executor,
    validation::Arguments,
};

/// Validated call at the last synchronous boundary before invoking user code.
#[derive(Clone, Copy, Debug)]
pub struct BeforeExecute<'a> {
    pub call_id: &'a str,
    pub name: &'a str,
    pub arguments: Arguments<'a>,
}
/// Execution returned bounded UTF-8 output. is_error=true is still a completed
/// business result, not an executor transport/contract error.
#[derive(Clone, Copy, Debug)]
pub struct AfterExecute<'a> {
    pub call: BeforeExecute<'a>,
    pub completion: Completion<'a, 'a>,
}
/// An attempted executor call failed or returned invalid output. Side effects
/// may already have happened; this observation does not authorize a retry.
#[derive(Clone, Copy, Debug)]
pub struct ExecuteError<'a> {
    pub call: BeforeExecute<'a>,
    pub error: Error,
}

impl<'a> Prepared<'a> {
    /// Gate the actual executor invocation, then observe exactly one validated
    /// result or error on normal return. Pause/deny never calls the executor or
    /// touches output and emits neither completion nor execution-error events.
    /// Panics are not caught; hooks and executors are trusted application code.
    pub fn execute_with_hooks<'o, E: Executor>(
        self,
        executor: &mut E,
        output: &'o mut [u8],
        hooks: &mut impl Hooks,
    ) -> Result<HookResult<Completion<'a, 'o>>, Error> {
        let call = BeforeExecute {
            call_id: self.call_id(),
            name: self.name(),
            arguments: self.arguments(),
        };
        match hooks.before_execute(call) {
            Decision::Allow => {}
            Decision::Pause => return Ok(HookResult::Paused),
            Decision::Deny => return Err(Error::Denied),
        }
        match self.execute(executor, output) {
            Ok(completion) => {
                hooks.after_execute(AfterExecute { call, completion });
                Ok(HookResult::Applied(completion))
            }
            Err(error) => {
                hooks.execute_error(ExecuteError { call, error });
                Err(error)
            }
        }
    }
}
