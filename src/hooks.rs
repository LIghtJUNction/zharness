//! Ordered, statically dispatched lifecycle hooks. No global callback registry.
//!
//! Before hooks can allow, pause or deny a proposed action. After hooks observe
//! committed state and cannot turn a completed side effect into a retry signal.
//! A paused before hook may be evaluated again; it must not execute the action.
use crate::{Error, Invocation, context::Group, zcall::v1::TurnOutcome};

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
pub trait Hooks {
    fn before(&mut self, _event: Before<'_>) -> Decision { Decision::Allow }
    fn after(&mut self, _event: After<'_>) {}
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
}
impl<H: Hooks + ?Sized> Hooks for &mut H {
    fn before(&mut self, event: Before<'_>) -> Decision { (**self).before(event) }
    fn after(&mut self, event: After<'_>) { (**self).after(event); }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub enum HookResult<T> { Applied(T), Paused }
pub(crate) fn permit(hooks: &mut impl Hooks, event: Before<'_>) -> Result<bool, Error> {
    match hooks.before(event) { Decision::Allow => Ok(true), Decision::Pause => Ok(false), Decision::Deny => Err(Error::Denied) }
}
