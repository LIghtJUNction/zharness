//! Lifecycle adapter for actual public zcall stream events, never vendor fields.
//! Gates apply before accepting an event into the application consumer. A Pause
//! rejects that event; retain/replay it via zcall and do not repeat side effects.
use crate::{
    Error,
    hooks::HookResult,
    lifecycle::{
        Correlation, Fault, Gate, Lifecycle, Observer, Outcome, Payload,
        domains::response::{Action, Event},
    },
    zcall::v1::{BlockType, Control, EventData, TurnOutcome},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    New,
    Receiving,
    Ended(TurnOutcome),
}
pub struct ResponseStream {
    correlation: Correlation,
    sequence: u64,
    state: State,
    visible: bool,
    finalized: bool,
}
impl ResponseStream {
    pub const fn new(correlation: Correlation) -> Self {
        Self {
            correlation,
            sequence: 0,
            state: State::New,
            visible: false,
            finalized: false,
        }
    }
    pub fn state(&self) -> State {
        self.state
    }
    pub fn accepted_events(&self) -> u64 {
        self.sequence
    }
    pub fn has_visible_content(&self) -> bool {
        self.visible
    }
    fn describe<'a>(&self, data: EventData<'a>, sequence: u64) -> Event<'a> {
        let mut event = Event::new(Action::TurnStart);
        event.sequence = sequence;
        match data {
            EventData::Started { .. } => {}
            EventData::IdentityUpdated(_) => event.action = Action::IdentityUpdated,
            EventData::BlockStarted { key, kind, .. } => {
                event.action = Action::BlockStart;
                event.key = Some(key);
                event.block = Some(kind);
            }
            EventData::Delta { key, kind, text } => {
                event.action = match kind {
                    BlockType::Text => Action::TextDelta,
                    BlockType::Reasoning => Action::ReasoningDelta,
                    BlockType::Tool => Action::ToolArgumentsDelta,
                    BlockType::Refusal => Action::RefusalDelta,
                };
                event.key = Some(key);
                event.block = Some(kind);
                event.payload = Payload::text(text);
                event.first_visible = !self.visible
                    && !event.payload.is_empty()
                    && matches!(kind, BlockType::Text | BlockType::Refusal);
            }
            EventData::Signature {
                key,
                signature,
                origin,
            } => {
                event.action = Action::Signature;
                event.key = Some(key);
                event.payload = Payload::text(signature);
                event.signature = Some(crate::zcall::v1::ProviderSignature {
                    source: origin,
                    value: signature,
                });
            }
            EventData::BlockEnded { key } => {
                event.action = Action::BlockEnd;
                event.key = Some(key);
            }
            EventData::ToolReady(tool) => {
                event.action = Action::ToolReady;
                event.key = Some(tool.key);
                event.block = Some(BlockType::Tool);
                event.payload = Payload::bytes(tool.arguments.as_bytes());
                event.signature = tool.signature;
            }
            EventData::Images { choice, json } => {
                event.action = Action::Images;
                event.choice = Some(choice);
                event.payload = Payload::bytes(json);
            }
            EventData::Grounding { choice, json } => {
                event.action = Action::Grounding;
                event.choice = Some(choice);
                event.payload = Payload::bytes(json);
            }
            EventData::Continuation { choice, token } => {
                event.action = Action::Continuation;
                event.choice = Some(choice);
                event.payload = Payload::text(token);
            }
            EventData::StopSequence { sequence } => {
                event.action = Action::StopSequence;
                event.payload = Payload::text(sequence);
            }
            EventData::Usage(usage) => {
                event.action = Action::Usage;
                event.usage = Some(usage);
            }
            EventData::ChoiceEnded { .. } => event.action = Action::ChoiceEnd,
            EventData::SnapshotVerified {
                key, kind, text, ..
            } => {
                event.action = Action::Snapshot;
                event.key = Some(key);
                event.block = Some(kind);
                event.payload = Payload::text(text);
            }
            EventData::BlockIncomplete { key } => {
                event.action = Action::BlockIncomplete;
                event.key = Some(key);
            }
            EventData::Loss { .. } => event.action = Action::Loss,
            EventData::Terminal(terminal) => {
                event.action = Action::TurnEnd;
                event.terminal = Some(terminal.outcome);
                event.usage = Some(terminal.usage);
            }
        }
        event
    }
    /// Consumer must not commit a side effect then return Pause. The wrapper
    /// rolls back only its own counters, not consumer/UI/history changes.
    /// An error (including Deny) is returned to the driver, which must explicitly
    /// stop/cancel the Session or safely retain the rejected event.
    pub fn accept<'a, G, O>(
        &mut self,
        data: EventData<'a>,
        lifecycle: &mut Lifecycle<G, O>,
        consume: impl FnOnce(EventData<'a>) -> Result<Control, Fault>,
    ) -> Result<Control, Fault>
    where
        G: Gate<Event<'a>>,
        O: Observer<Event<'a>>,
    {
        if matches!(self.state, State::Ended(_)) {
            return Err(Error::InvalidState.into());
        }
        if matches!(data, EventData::Started { .. }) && self.state != State::New {
            return Err(Error::InvalidState.into());
        }
        if self.state == State::New
            && !matches!(data, EventData::Started { .. } | EventData::Terminal(_))
        {
            return Err(Error::InvalidState.into());
        }
        let sequence = self.sequence.checked_add(1).ok_or(Error::Overflow)?;
        let event = self.describe(data, sequence);
        let HookResult::Applied(mut span) = lifecycle.begin(self.correlation, event)? else {
            return Ok(Control::Pause);
        };
        match consume(data) {
            Ok(Control::Continue) => {
                self.sequence = sequence;
                self.visible |= event.first_visible;
                self.state = match event.terminal {
                    Some(outcome) => State::Ended(outcome),
                    None => State::Receiving,
                };
                // Success means event accepted, not that a failed upstream turn
                // became successful. The actual terminal outcome stays in Event.
                lifecycle.finish(&mut span, Outcome::Succeeded)?;
                Ok(Control::Continue)
            }
            Ok(Control::Pause) => {
                lifecycle.finish(&mut span, Outcome::Deferred)?;
                Ok(Control::Pause)
            }
            Err(error) => {
                lifecycle.finish(&mut span, Outcome::Failed(error))?;
                Err(error)
            }
        }
    }
    /// Explicit final assistant-message acceptance, separate from per-turn end.
    /// WaitingTools, incomplete and failed turns cannot masquerade as final answers.
    pub fn finalize<'a, G, O, R>(
        &mut self,
        content: Payload<'a>,
        lifecycle: &mut Lifecycle<G, O>,
        commit: impl FnOnce() -> Result<R, Fault>,
    ) -> Result<HookResult<R>, Fault>
    where
        G: Gate<Event<'a>>,
        O: Observer<Event<'a>>,
    {
        if self.state != State::Ended(TurnOutcome::Completed) || self.finalized {
            return Err(Error::InvalidState.into());
        }
        let mut event = Event::new(Action::FinalizeAssistant);
        event.payload = content;
        event.sequence = self.sequence;
        lifecycle.run(self.correlation, event, |_| {
            let result = commit()?;
            self.finalized = true;
            Ok(result)
        })
    }
    /// Logical cancellation only; external response reading must be stopped by
    /// the caller. Already-ended responses cannot be cancelled retroactively.
    pub fn cancel<G, O>(&mut self, lifecycle: &mut Lifecycle<G, O>) -> Result<HookResult<()>, Fault>
    where
        G: Gate<Event<'static>>,
        O: Observer<Event<'static>>,
    {
        if matches!(self.state, State::Ended(_)) {
            return Err(Error::InvalidState.into());
        }
        lifecycle.run(self.correlation, Event::new(Action::Cancel), |_| {
            self.state = State::Ended(TurnOutcome::Cancelled);
            Ok(())
        })
    }
}
