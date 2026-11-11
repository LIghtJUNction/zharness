//! Ordered, statically dispatched lifecycle hooks. No global registry.
//! Before callbacks gate proposals; after/error callbacks observe and never veto
//! completed effects. Hook/adapter panics are not caught by this no_std core.
use crate::{
    Error, Invocation,
    command::{Command, CommandFailure, CommandResult},
    context::Group,
    execution::{AfterExecute, BeforeExecute, ExecuteError},
    output::ResultMeta,
    zcall::v1::TurnOutcome,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub enum Decision {
    Allow,
    Pause,
    Deny,
}
#[derive(Clone, Copy, Debug)]
pub enum Before<'a> {
    Context {
        group: Group<'a>,
        compacting: bool,
    },
    Model {
        run: u64,
        round: u32,
    },
    Tool {
        run: u64,
        round: u32,
        call: Invocation<'a>,
    },
    Assignment {
        task: u64,
        agent: u64,
        attempt: u64,
    },
}
#[derive(Clone, Copy, Debug)]
pub enum After<'a> {
    ContextChanged {
        groups: usize,
        tokens: u64,
    },
    ModelFinished {
        run: u64,
        round: u32,
        outcome: TurnOutcome,
    },
    ToolFinished {
        run: u64,
        round: u32,
        call_id: &'a str,
        is_error: bool,
    },
    Stopped {
        run: u64,
    },
    Assigned {
        task: u64,
        agent: u64,
        attempt: u64,
    },
    TaskFinished {
        task: u64,
        agent: u64,
        attempt: u64,
        is_error: bool,
    },
    LeaseExpired {
        task: u64,
        agent: u64,
        attempt: u64,
    },
    TaskCancelled {
        task: u64,
    },
}
#[derive(Clone, Copy, Debug)]
pub struct BeforeResult<'a> {
    pub meta: ResultMeta<'a>,
    pub text: &'a str,
}
#[derive(Clone, Copy, Debug)]
pub struct AfterResult<'a> {
    pub meta: ResultMeta<'a>,
    pub original_bytes: usize,
    pub text: &'a str,
}

pub trait Hooks {
    fn before(&mut self, _event: Before<'_>) -> Decision {
        Decision::Allow
    }
    fn after(&mut self, _event: After<'_>) {}
    fn before_result(&mut self, _event: BeforeResult<'_>) -> Decision {
        Decision::Allow
    }
    fn after_result(&mut self, _event: AfterResult<'_>) {}
    /// Gate actual validated tool execution, not just a ToolLoop ticket issuance.
    fn before_execute(&mut self, _event: BeforeExecute<'_>) -> Decision {
        Decision::Allow
    }
    fn after_execute(&mut self, _event: AfterExecute<'_>) {}
    fn execute_error(&mut self, _event: ExecuteError<'_>) {}
    /// Gate a command adapter call with its exact executable, argv and cwd.
    fn before_command(&mut self, _command: Command<'_>) -> Decision {
        Decision::Allow
    }
    fn after_command(&mut self, _result: CommandResult<'_, '_>) {}
    fn command_error(&mut self, _event: CommandFailure<'_>) {}
}
#[derive(Clone, Copy, Debug, Default)]
pub struct NoHooks;
impl Hooks for NoHooks {}
/// Tuple chains short-circuit gates left-to-right. All normal-return observers
/// run in that same order; they cannot change the returned result or error.
impl<A: Hooks, B: Hooks> Hooks for (A, B) {
    fn before(&mut self, event: Before<'_>) -> Decision {
        match self.0.before(event) {
            Decision::Allow => self.1.before(event),
            d => d,
        }
    }
    fn after(&mut self, event: After<'_>) {
        self.0.after(event);
        self.1.after(event);
    }
    fn before_result(&mut self, event: BeforeResult<'_>) -> Decision {
        match self.0.before_result(event) {
            Decision::Allow => self.1.before_result(event),
            d => d,
        }
    }
    fn after_result(&mut self, event: AfterResult<'_>) {
        self.0.after_result(event);
        self.1.after_result(event);
    }
    fn before_execute(&mut self, event: BeforeExecute<'_>) -> Decision {
        match self.0.before_execute(event) {
            Decision::Allow => self.1.before_execute(event),
            d => d,
        }
    }
    fn after_execute(&mut self, event: AfterExecute<'_>) {
        self.0.after_execute(event);
        self.1.after_execute(event);
    }
    fn execute_error(&mut self, event: ExecuteError<'_>) {
        self.0.execute_error(event);
        self.1.execute_error(event);
    }
    fn before_command(&mut self, command: Command<'_>) -> Decision {
        match self.0.before_command(command) {
            Decision::Allow => self.1.before_command(command),
            d => d,
        }
    }
    fn after_command(&mut self, result: CommandResult<'_, '_>) {
        self.0.after_command(result);
        self.1.after_command(result);
    }
    fn command_error(&mut self, event: CommandFailure<'_>) {
        self.0.command_error(event);
        self.1.command_error(event);
    }
}
impl<H: Hooks + ?Sized> Hooks for &mut H {
    fn before(&mut self, event: Before<'_>) -> Decision {
        (**self).before(event)
    }
    fn after(&mut self, event: After<'_>) {
        (**self).after(event);
    }
    fn before_result(&mut self, event: BeforeResult<'_>) -> Decision {
        (**self).before_result(event)
    }
    fn after_result(&mut self, event: AfterResult<'_>) {
        (**self).after_result(event);
    }
    fn before_execute(&mut self, event: BeforeExecute<'_>) -> Decision {
        (**self).before_execute(event)
    }
    fn after_execute(&mut self, event: AfterExecute<'_>) {
        (**self).after_execute(event);
    }
    fn execute_error(&mut self, event: ExecuteError<'_>) {
        (**self).execute_error(event);
    }
    fn before_command(&mut self, command: Command<'_>) -> Decision {
        (**self).before_command(command)
    }
    fn after_command(&mut self, result: CommandResult<'_, '_>) {
        (**self).after_command(result);
    }
    fn command_error(&mut self, event: CommandFailure<'_>) {
        (**self).command_error(event);
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub enum HookResult<T> {
    Applied(T),
    Paused,
}
pub(crate) fn permit(hooks: &mut impl Hooks, event: Before<'_>) -> Result<bool, Error> {
    match hooks.before(event) {
        Decision::Allow => Ok(true),
        Decision::Pause => Ok(false),
        Decision::Deny => Err(Error::Denied),
    }
}
