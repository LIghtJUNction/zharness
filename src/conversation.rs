//! Hooked acceptance of application conversation operations. No history database,
//! UI listener or message deduplication store is hidden inside this controller.
use crate::{
    Error,
    hooks::HookResult,
    lifecycle::{
        Correlation, Fault, Gate, Lifecycle, Observer, Payload,
        domains::conversation::{Action, Event},
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    New,
    Open,
    Closed,
}
pub struct Conversation {
    correlation: Correlation,
    state: State,
    revision: u64,
}
impl Conversation {
    pub const fn new(correlation: Correlation) -> Self {
        Self {
            correlation,
            state: State::New,
            revision: 0,
        }
    }
    pub fn state(&self) -> State {
        self.state
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn correlation(&self) -> Correlation {
        self.correlation
    }
    pub fn start<G, O, R>(
        &mut self,
        lifecycle: &mut Lifecycle<G, O>,
        create: impl FnOnce() -> Result<R, Fault>,
    ) -> Result<HookResult<R>, Fault>
    where
        G: Gate<Event<'static>>,
        O: Observer<Event<'static>>,
    {
        if self.state != State::New {
            return Err(Error::InvalidState.into());
        }
        lifecycle.run(self.correlation, Event::new(Action::Start), |_| {
            let result = create()?;
            self.state = State::Open;
            Ok(result)
        })
    }
    pub fn close<G, O, R>(
        &mut self,
        lifecycle: &mut Lifecycle<G, O>,
        close: impl FnOnce() -> Result<R, Fault>,
    ) -> Result<HookResult<R>, Fault>
    where
        G: Gate<Event<'static>>,
        O: Observer<Event<'static>>,
    {
        if self.state != State::Open {
            return Err(Error::InvalidState.into());
        }
        lifecycle.run(
            self.correlation,
            Event::new(Action::Close).at_revision(self.revision),
            |_| {
                let result = close()?;
                self.state = State::Closed;
                Ok(result)
            },
        )
    }
    fn message<'a, G, O, R>(
        &mut self,
        id: u64,
        action: Action,
        content: Payload<'a>,
        lifecycle: &mut Lifecycle<G, O>,
        apply: impl FnOnce(Payload<'a>) -> Result<R, Fault>,
    ) -> Result<HookResult<R>, Fault>
    where
        G: Gate<Event<'a>>,
        O: Observer<Event<'a>>,
    {
        if self.state != State::Open {
            return Err(Error::InvalidState.into());
        }
        let revision = self.revision.checked_add(1).ok_or(Error::Overflow)?;
        let correlation = Correlation {
            message: Some(id),
            ..self.correlation
        };
        lifecycle.run(
            correlation,
            Event::new(action)
                .with_payload(content)
                .at_revision(self.revision),
            |event| {
                let result = apply(event.payload)?;
                self.revision = revision;
                Ok(result)
            },
        )
    }
    /// Call only after the application's transport/UI actually provides the input.
    /// The supplied operation owns validation and durable/history acceptance.
    pub fn user_message<'a, G, O, R>(
        &mut self,
        id: u64,
        content: Payload<'a>,
        lifecycle: &mut Lifecycle<G, O>,
        accept: impl FnOnce(Payload<'a>) -> Result<R, Fault>,
    ) -> Result<HookResult<R>, Fault>
    where
        G: Gate<Event<'a>>,
        O: Observer<Event<'a>>,
    {
        self.message(id, Action::AcceptUser, content, lifecycle, accept)
    }
    pub fn append_message<'a, G, O, R>(
        &mut self,
        id: u64,
        content: Payload<'a>,
        lifecycle: &mut Lifecycle<G, O>,
        append: impl FnOnce(Payload<'a>) -> Result<R, Fault>,
    ) -> Result<HookResult<R>, Fault>
    where
        G: Gate<Event<'a>>,
        O: Observer<Event<'a>>,
    {
        self.message(id, Action::AppendMessage, content, lifecycle, append)
    }
    pub fn edit_message<'a, G, O, R>(
        &mut self,
        id: u64,
        content: Payload<'a>,
        lifecycle: &mut Lifecycle<G, O>,
        edit: impl FnOnce(Payload<'a>) -> Result<R, Fault>,
    ) -> Result<HookResult<R>, Fault>
    where
        G: Gate<Event<'a>>,
        O: Observer<Event<'a>>,
    {
        self.message(id, Action::EditMessage, content, lifecycle, edit)
    }
    pub fn delete_message<G, O, R>(
        &mut self,
        id: u64,
        lifecycle: &mut Lifecycle<G, O>,
        delete: impl FnOnce() -> Result<R, Fault>,
    ) -> Result<HookResult<R>, Fault>
    where
        G: Gate<Event<'static>>,
        O: Observer<Event<'static>>,
    {
        self.message(
            id,
            Action::DeleteMessage,
            Payload::metadata(0),
            lifecycle,
            |_| delete(),
        )
    }
}
