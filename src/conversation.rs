//! Hooked acceptance of application conversation operations. No history database,
//! UI listener or message deduplication store is hidden inside this controller.
use crate::{
    Error,
    hooks::HookResult,
    lifecycle::{
        Correlation, Fault, Gate, Interceptor, Lifecycle, NoInterceptor, Observer, Payload,
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
struct ContentFilter<'i, I>(&'i mut I);
impl<'a, I: Interceptor<Payload<'a>>> Interceptor<Event<'a>> for ContentFilter<'_, I> {
    fn intercept(&mut self, event: &mut Event<'a>) -> Result<(), Fault> {
        self.0.intercept(&mut event.payload)
    }
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
        event: Event<'a>,
        filter: &mut impl Interceptor<Payload<'a>>,
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
        lifecycle.run_intercepted(
            correlation,
            event.at_revision(self.revision),
            &mut ContentFilter(filter),
            |event| {
                let result = apply(event.payload)?;
                self.revision = revision;
                Ok(result)
            },
        )
    }
    /// Call only after the application's transport/UI actually supplies input.
    /// The operation owns validation, deduplication and durable/history acceptance.
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
        self.user_message_intercepted(id, content, &mut NoInterceptor, lifecycle, accept)
    }
    /// Only candidate content can be rewritten; IDs, action and revision remain
    /// authoritative. Gate and acceptance receive the same final payload.
    pub fn user_message_intercepted<'a, G, O, R>(
        &mut self,
        id: u64,
        content: Payload<'a>,
        filter: &mut impl Interceptor<Payload<'a>>,
        lifecycle: &mut Lifecycle<G, O>,
        accept: impl FnOnce(Payload<'a>) -> Result<R, Fault>,
    ) -> Result<HookResult<R>, Fault>
    where
        G: Gate<Event<'a>>,
        O: Observer<Event<'a>>,
    {
        self.message(
            id,
            Event::new(Action::AcceptUser).with_payload(content),
            filter,
            lifecycle,
            accept,
        )
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
        self.message(
            id,
            Event::new(Action::AppendMessage).with_payload(content),
            &mut NoInterceptor,
            lifecycle,
            append,
        )
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
        self.message(
            id,
            Event::new(Action::EditMessage).with_payload(content),
            &mut NoInterceptor,
            lifecycle,
            edit,
        )
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
            Event::new(Action::DeleteMessage),
            &mut NoInterceptor,
            lifecycle,
            |_| delete(),
        )
    }
}
