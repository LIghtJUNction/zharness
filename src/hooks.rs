//! Ordered, statically dispatched lifecycle hooks. No global callback registry.
//!
//! Before hooks can allow, pause or deny a proposed action. After hooks observe
//! committed state and cannot turn a completed side effect into a retry signal.
//! A paused before hook may be evaluated again; it must not execute the action.
use crate::{Error, Invocation, context::Group, output::ResultMeta, zcall::v1::TurnOutcome};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub enum Decision { Allow, Pause, Deny }
#[derive(Clone, Copy, Debug)]
pub enum Before<'a> {
    Context { group: Group<'a>, compacting: bool },
    Model { run: u64, round: u32 },
    Tool { run: u64, round: u32, call: Invocation<'a> },
    Assignment { task: u64, agent: u64, attempt: u64 },
}
#[derive(Clone, Copy, Debug)]
pub enum After<'a> {
    ContextChanged { groups: usize, tokens: u64 },
    ModelFinished { run: u64, round: u32, outcome: TurnOutcome },
    ToolFinished { run: u64, round: u32, call_id: &'a str, is_error: bool },
    Stopped { run: u64 },
    Assigned { task: u64, agent: u64, attempt: u64 },
    TaskFinished { task: u64, agent: u64, attempt: u64, is_error: bool },
    LeaseExpired { task: u64, agent: u64, attempt: u64 },
    TaskCancelled { task: u64 },
}

/// A result-processing proposal, after tool execution but before history commit.
/// Pause/deny retains original output and does not execute a transform.
#[derive(Clone, Copy, Debug)]
pub struct BeforeResult<'a> { pub meta: ResultMeta<'a>, pub text: &'a str }
/// A validated transform output. This observes processing, not ToolLoop/history
/// commitment. It cannot veto the result. Avoid leaking secrets into audit logs.
#[derive(Clone, Copy, Debug)]
pub struct AfterResult<'a> { pub meta: ResultMeta<'a>, pub original_bytes: usize, pub text: &'a str }

pub trait Hooks {
    fn before(&mut self, _event: Before<'_>) -> Decision { Decision::Allow }
    fn after(&mut self, _event: After<'_>) {}
    /// Additive default method; existing lifecycle implementations remain valid.
    fn before_result(&mut self, _event: BeforeResult<'_>) -> Decision { Decision::Allow }
    fn after_result(&mut self, _event: AfterResult<'_>) {}
}
#[derive(Clone, Copy, Debug, Default)]
pub struct NoHooks;
impl Hooks for NoHooks {}
/// Compose heterogeneous hooks with `(audit, policy)` or nested tuples.
/// Before hooks short-circuit left-to-right. After observers all run in that order.
impl<A: Hooks, B: Hooks> Hooks for (A, B) {
    fn before(&mut self, event: Before<'_>) -> Decision {
        match self.0.before(event) { Decision::Allow => self.1.before(event), decision => decision }
    }
    fn after(&mut self, event: After<'_>) { self.0.after(event); self.1.after(event); }
    fn before_result(&mut self, event: BeforeResult<'_>) -> Decision {
        match self.0.before_result(event) { Decision::Allow => self.1.before_result(event), decision => decision }
    }
    fn after_result(&mut self, event: AfterResult<'_>) { self.0.after_result(event); self.1.after_result(event); }
}
impl<H: Hooks + ?Sized> Hooks for &mut H {
    fn before(&mut self, event: Before<'_>) -> Decision { (**self).before(event) }
    fn after(&mut self, event: After<'_>) { (**self).after(event); }
    fn before_result(&mut self, event: BeforeResult<'_>) -> Decision { (**self).before_result(event) }
    fn after_result(&mut self, event: AfterResult<'_>) { (**self).after_result(event); }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub enum HookResult<T> { Applied(T), Paused }
pub(crate) fn permit(hooks: &mut impl Hooks, event: Before<'_>) -> Result<bool, Error> {
    match hooks.before(event) { Decision::Allow => Ok(true), Decision::Pause => Ok(false), Decision::Deny => Err(Error::Denied) }
}
