//! A bounded model/tool loop controller, not an automatic agent runner.
//!
//! `next` issues one action. The application performs it and feeds observations
//! back. ToolReady events are retained during streaming; tools are not scheduled
//! until zcall confirms the model turn is WaitingTools. No I/O occurs here.
use crate::{
    Error, Invocation,
    bridge::{Completion, InvocationStorage},
    hooks::{self, After, Before, Hooks},
    zcall::v1::{EventData, TurnOutcome},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LoopLimits {
    pub max_rounds: u32,
    pub max_tool_calls: u64,
}
impl Default for LoopLimits {
    fn default() -> Self {
        Self {
            max_rounds: 16,
            max_tool_calls: 128,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Stop {
    Completed,
    Cancelled,
    RoundLimit,
    Model(TurnOutcome),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    Ready,
    Receiving,
    Tools,
    Stopped(Stop),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CallState {
    Empty,
    Ready,
    Running,
    Complete,
}

/// Caller-owned retention. One slot per outstanding call, including completed
/// calls until the next model action is accepted. Clearing is not secure erasure.
pub struct CallSlot<const N: usize> {
    call: InvocationStorage<N>,
    state: CallState,
}
impl<const N: usize> CallSlot<N> {
    pub const fn new() -> Self {
        Self {
            call: InvocationStorage::new(),
            state: CallState::Empty,
        }
    }
    fn clear(&mut self) {
        self.call.clear();
        self.state = CallState::Empty;
    }
}
impl<const N: usize> Default for CallSlot<N> {
    fn default() -> Self {
        Self::new()
    }
}

/// A dispatch identity, not a provider tool ID. Private fields prevent inventing
/// a completion for an unissued slot. Use a distinct run ID per application run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Ticket {
    run: u64,
    round: u32,
    slot: usize,
}
impl Ticket {
    pub fn run(self) -> u64 {
        self.run
    }
    pub fn round(self) -> u32 {
        self.round
    }
    pub fn slot(self) -> usize {
        self.slot
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub enum Action {
    CallModel {
        run: u64,
        round: u32,
    },
    Execute(Ticket),
    /// Receiving model data or waiting for already-dispatched tools.
    Pending,
    Paused,
    Stopped(Stop),
}

pub struct ToolLoop<'s, const N: usize> {
    run: u64,
    slots: &'s mut [CallSlot<N>],
    used: usize,
    round: u32,
    total_calls: u64,
    limits: LoopLimits,
    state: State,
}
impl<'s, const N: usize> ToolLoop<'s, N> {
    pub fn new(run: u64, slots: &'s mut [CallSlot<N>], limits: LoopLimits) -> Self {
        for slot in slots.iter_mut() {
            slot.clear();
        }
        Self {
            run,
            slots,
            used: 0,
            round: 0,
            total_calls: 0,
            limits,
            state: State::Ready,
        }
    }
    pub fn state(&self) -> State {
        self.state
    }
    pub fn round(&self) -> u32 {
        self.round
    }
    pub fn total_calls(&self) -> u64 {
        self.total_calls
    }
    pub fn retained_calls(&self) -> usize {
        self.used
    }
    pub fn memory_bytes(&self) -> usize {
        core::mem::size_of::<Self>() + core::mem::size_of_val(self.slots)
    }
    fn stop(&mut self, reason: Stop, hooks: &mut impl Hooks) -> Action {
        self.state = State::Stopped(reason);
        hooks.after(After::Stopped { run: self.run });
        Action::Stopped(reason)
    }
    fn begin_model(&mut self, hooks: &mut impl Hooks) -> Result<Action, Error> {
        if self.round >= self.limits.max_rounds {
            return Ok(self.stop(Stop::RoundLimit, hooks));
        }
        let round = self.round.checked_add(1).ok_or(Error::Overflow)?;
        if !hooks::permit(
            hooks,
            Before::Model {
                run: self.run,
                round,
            },
        )? {
            return Ok(Action::Paused);
        }
        for slot in &mut self.slots[..self.used] {
            slot.clear();
        }
        self.used = 0;
        self.round = round;
        self.state = State::Receiving;
        Ok(Action::CallModel {
            run: self.run,
            round,
        })
    }
    /// Before hooks run before state commits. Pause/deny does not dispatch work.
    /// Multiple Execute tickets may be issued before any results are committed.
    pub fn next(&mut self, hooks: &mut impl Hooks) -> Result<Action, Error> {
        match self.state {
            State::Ready => self.begin_model(hooks),
            State::Receiving => Ok(Action::Pending),
            State::Stopped(reason) => Ok(Action::Stopped(reason)),
            State::Tools => {
                if let Some(at) = self.slots[..self.used]
                    .iter()
                    .position(|s| s.state == CallState::Ready)
                {
                    let call = self.slots[at].call.view().ok_or(Error::InvalidState)?;
                    if !hooks::permit(
                        hooks,
                        Before::Tool {
                            run: self.run,
                            round: self.round,
                            call,
                        },
                    )? {
                        return Ok(Action::Paused);
                    }
                    self.slots[at].state = CallState::Running;
                    Ok(Action::Execute(Ticket {
                        run: self.run,
                        round: self.round,
                        slot: at,
                    }))
                } else if self.slots[..self.used]
                    .iter()
                    .all(|s| s.state == CallState::Complete)
                {
                    self.begin_model(hooks)
                } else {
                    Ok(Action::Pending)
                }
            }
        }
    }
    fn retain(&mut self, call: Invocation<'_>) -> Result<(), Error> {
        if self.total_calls >= self.limits.max_tool_calls {
            return Err(Error::LimitExceeded);
        }
        if self.slots[..self.used]
            .iter()
            .any(|s| s.call.view().is_some_and(|old| old.call_id == call.call_id))
        {
            return Err(Error::DuplicateId);
        }
        let needed = self.used.checked_add(1).ok_or(Error::Overflow)?;
        let slot = self
            .slots
            .get_mut(self.used)
            .ok_or(Error::Capacity { needed })?;
        let total = self.total_calls.checked_add(1).ok_or(Error::Overflow)?;
        slot.call.store(call)?;
        slot.state = CallState::Ready;
        self.used = needed;
        self.total_calls = total;
        Ok(())
    }
    /// Return Control::Continue to zcall only after this succeeds. A recoverable
    /// capacity failure accepts no call: return Pause and retain the event for retry.
    /// On policy/limit failures the application should explicitly cancel/abort.
    pub fn observe(&mut self, event: EventData<'_>, hooks: &mut impl Hooks) -> Result<(), Error> {
        if self.state != State::Receiving {
            return Err(Error::InvalidState);
        }
        match event {
            EventData::ToolReady(tool) => self.retain(Invocation::from_zcall(tool)),
            EventData::Terminal(terminal) => {
                let next = match terminal.outcome {
                    TurnOutcome::WaitingTools if self.used != 0 => State::Tools,
                    TurnOutcome::WaitingTools => return Err(Error::InvalidState),
                    TurnOutcome::Completed if self.used == 0 => State::Stopped(Stop::Completed),
                    TurnOutcome::Completed => return Err(Error::InvalidState),
                    TurnOutcome::Cancelled => State::Stopped(Stop::Cancelled),
                    other => State::Stopped(Stop::Model(other)),
                };
                self.state = next;
                hooks.after(After::ModelFinished {
                    run: self.run,
                    round: self.round,
                    outcome: terminal.outcome,
                });
                if matches!(next, State::Stopped(_)) {
                    hooks.after(After::Stopped { run: self.run });
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
    fn index(&self, ticket: Ticket) -> Result<usize, Error> {
        if ticket.run != self.run || ticket.round != self.round || ticket.slot >= self.used {
            return Err(Error::Stale);
        }
        if !matches!(
            self.slots[ticket.slot].state,
            CallState::Running | CallState::Complete
        ) {
            return Err(Error::Stale);
        }
        Ok(ticket.slot)
    }
    /// Borrow until the next mutable controller operation. Copy to application
    /// history storage when the call must survive the next model turn.
    pub fn invocation(&self, ticket: Ticket) -> Result<Invocation<'_>, Error> {
        let at = self.index(ticket)?;
        self.slots[at].call.view().ok_or(Error::Stale)
    }
    /// Call after retaining the result and accepting it into your history/session.
    /// This validates the real call ID and rejects duplicate/stale completions.
    /// It does not append history or submit to a zcall Session behind your back.
    pub fn commit_result(
        &mut self,
        ticket: Ticket,
        result: &Completion<'_, '_>,
        hooks: &mut impl Hooks,
    ) -> Result<(), Error> {
        if self.state != State::Tools {
            return Err(Error::InvalidState);
        }
        let at = self.index(ticket)?;
        if self.slots[at].state != CallState::Running {
            return Err(Error::Stale);
        }
        if self.slots[at]
            .call
            .view()
            .is_none_or(|call| call.call_id != result.call_id)
        {
            return Err(Error::UnknownId);
        }
        self.slots[at].state = CallState::Complete;
        hooks.after(After::ToolFinished {
            run: self.run,
            round: self.round,
            call_id: result.call_id,
            is_error: result.is_error,
        });
        Ok(())
    }
    /// Logical cancellation only; the application must stop outstanding I/O/work.
    pub fn cancel(&mut self, hooks: &mut impl Hooks) -> bool {
        if matches!(self.state, State::Stopped(_)) {
            return false;
        }
        let _ = self.stop(Stop::Cancelled, hooks);
        true
    }
    pub fn set_limits(&mut self, limits: LoopLimits) -> Result<(), Error> {
        if limits.max_rounds < self.round || limits.max_tool_calls < self.total_calls {
            return Err(Error::LimitExceeded);
        }
        self.limits = limits;
        Ok(())
    }
    /// Grow the number of slots using caller memory. Existing calls and tickets
    /// are preserved; too-small storage is rejected without changing either side.
    pub fn replace_slots(&mut self, slots: &'s mut [CallSlot<N>]) -> Result<(), Error> {
        if slots.len() < self.used {
            return Err(Error::Capacity { needed: self.used });
        }
        for slot in slots.iter_mut() {
            slot.clear();
        }
        for (old, new) in self.slots[..self.used].iter_mut().zip(slots.iter_mut()) {
            core::mem::swap(old, new);
        }
        self.slots = slots;
        Ok(())
    }
}
