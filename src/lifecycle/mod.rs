//! Typed, independent lifecycle channels. No global registry or external runtime.
//!
//! Gate decides, Interceptor edits a copied candidate, Observer records facts.
//! Pair actual application operations through `run`, or use `begin`/`finish`
//! around asynchronous work. Merely declaring an event does not perform work.
use crate::{
    Error,
    hooks::{Decision, HookResult},
    zcall::v1::Text,
};

pub mod domains;

/// Correlation metadata, supplied by the application, never rewritten by a filter.
/// Use distinct epochs/runs after restart. IDs are not authorization credentials.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Correlation {
    pub conversation: u64,
    pub run: u64,
    pub request: Option<u64>,
    pub message: Option<u64>,
    pub agent: Option<u64>,
    pub attempt: u32,
}

/// Bounded failures. Rich application diagnostics stay outside event metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Fault {
    Core(Error),
    Application(u32),
    Timeout,
    Cancelled,
}
impl From<Error> for Fault {
    fn from(error: Error) -> Self {
        Self::Core(error)
    }
}
impl core::fmt::Display for Fault {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl core::error::Error for Fault {}

#[derive(Clone, Copy)]
enum Body<'a> {
    Bytes(&'a [u8]),
    Text(Text<'a>),
}

/// Borrowed content with a redacted Debug implementation. Observers receive only
/// length by default; explicit Visibility::Content permits access to supplied data.
#[derive(Clone, Copy)]
pub struct Payload<'a> {
    bytes: usize,
    body: Option<Body<'a>>,
}
impl<'a> Payload<'a> {
    pub const fn metadata(bytes: usize) -> Self {
        Self { bytes, body: None }
    }
    pub const fn bytes(bytes: &'a [u8]) -> Self {
        Self {
            bytes: bytes.len(),
            body: Some(Body::Bytes(bytes)),
        }
    }
    pub fn text(text: Text<'a>) -> Self {
        Self {
            bytes: text.decoded_len(),
            body: Some(Body::Text(text)),
        }
    }
    pub fn utf8(text: &'a str) -> Self {
        Self::text(Text::new(text))
    }
    pub const fn len(self) -> usize {
        self.bytes
    }
    pub const fn is_empty(self) -> bool {
        self.bytes == 0
    }
    pub const fn redacted(self) -> Self {
        Self::metadata(self.bytes)
    }
    pub const fn as_bytes(self) -> Option<&'a [u8]> {
        match self.body {
            Some(Body::Bytes(bytes)) => Some(bytes),
            _ => None,
        }
    }
    /// Text may be escaped wire JSON; decode through zcall Text when necessary.
    pub const fn as_text(self) -> Option<Text<'a>> {
        match self.body {
            Some(Body::Text(text)) => Some(text),
            _ => None,
        }
    }
}
impl core::fmt::Debug for Payload<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Payload")
            .field("bytes", &self.bytes)
            .field("content_supplied", &self.body.is_some())
            .finish()
    }
}

/// Implement for custom channels. `redacted` MUST strip sensitive content from
/// observer views. Built-in domains implement it without retaining data pointers.
pub trait Event: Copy {
    fn redacted(self) -> Self;
    fn validate(self) -> Result<(), Fault> {
        Ok(())
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Proposal<E> {
    /// Monotonic proposal serial, including pause/deny/rejected attempts.
    pub serial: u64,
    pub correlation: Correlation,
    pub event: E,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    Succeeded,
    Failed(Fault),
    Cancelled,
    Abandoned,
    Deferred,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Started,
    Finished(Outcome),
    Paused,
    Denied,
    Rejected(Fault),
    /// A fact already observed externally; it is not a vetoable proposal.
    Observed,
}
#[derive(Clone, Copy, Debug)]
pub struct Notice<E> {
    pub proposal: Proposal<E>,
    pub phase: Phase,
}

pub trait Gate<E: Event> {
    fn before(&mut self, _proposal: Proposal<E>) -> Decision {
        Decision::Allow
    }
}
pub trait Observer<E: Event> {
    fn observe(&mut self, _notice: Notice<E>) {}
}
/// Edits only a copied event/candidate. Validate and gate the FINAL candidate;
/// returning an error must never authorize fallback to the original content.
pub trait Interceptor<T> {
    fn intercept(&mut self, candidate: &mut T) -> Result<(), Fault>;
}
#[derive(Clone, Copy, Debug, Default)]
pub struct NoGate;
impl<E: Event> Gate<E> for NoGate {}
#[derive(Clone, Copy, Debug, Default)]
pub struct NoObserver;
impl<E: Event> Observer<E> for NoObserver {}
#[derive(Clone, Copy, Debug, Default)]
pub struct NoInterceptor;
impl<T> Interceptor<T> for NoInterceptor {
    fn intercept(&mut self, _: &mut T) -> Result<(), Fault> {
        Ok(())
    }
}
impl<E: Event, A: Gate<E>, B: Gate<E>> Gate<E> for (A, B) {
    fn before(&mut self, proposal: Proposal<E>) -> Decision {
        match self.0.before(proposal) {
            Decision::Allow => self.1.before(proposal),
            other => other,
        }
    }
}
impl<E: Event, A: Observer<E>, B: Observer<E>> Observer<E> for (A, B) {
    fn observe(&mut self, notice: Notice<E>) {
        self.0.observe(notice);
        self.1.observe(notice);
    }
}
impl<T, A: Interceptor<T>, B: Interceptor<T>> Interceptor<T> for (A, B) {
    fn intercept(&mut self, candidate: &mut T) -> Result<(), Fault> {
        self.0.intercept(candidate)?;
        self.1.intercept(candidate)
    }
}
impl<E: Event, G: Gate<E> + ?Sized> Gate<E> for &mut G {
    fn before(&mut self, proposal: Proposal<E>) -> Decision {
        (**self).before(proposal)
    }
}
impl<E: Event, O: Observer<E> + ?Sized> Observer<E> for &mut O {
    fn observe(&mut self, notice: Notice<E>) {
        (**self).observe(notice);
    }
}
impl<T, I: Interceptor<T> + ?Sized> Interceptor<T> for &mut I {
    fn intercept(&mut self, candidate: &mut T) -> Result<(), Fault> {
        (**self).intercept(candidate)
    }
}

/// Functional adapters avoid requiring a callback object or heap allocation.
pub struct GateFn<F>(pub F);
impl<E: Event, F: FnMut(Proposal<E>) -> Decision> Gate<E> for GateFn<F> {
    fn before(&mut self, proposal: Proposal<E>) -> Decision {
        (self.0)(proposal)
    }
}
pub struct ObserveFn<F>(pub F);
impl<E: Event, F: FnMut(Notice<E>)> Observer<E> for ObserveFn<F> {
    fn observe(&mut self, notice: Notice<E>) {
        (self.0)(notice);
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Visibility {
    #[default]
    Metadata,
    Content,
}

/// A non-cloneable operation receipt. Keep borrowed event data alive until finish.
/// Dropping it does not cancel external work or invent a completion observation.
#[derive(Debug)]
#[must_use = "explicitly finish, fail, defer or abandon this operation"]
pub struct Span<E: Event> {
    epoch: u64,
    proposal: Proposal<E>,
    finished: bool,
}
impl<E: Event> Span<E> {
    pub fn proposal(&self) -> Proposal<E> {
        self.proposal
    }
    pub fn is_finished(&self) -> bool {
        self.finished
    }
}

/// Stateless with respect to the application; proportional in-flight storage is
/// held by caller-owned Span values. One serialized coordinator per instance.
pub struct Lifecycle<G = NoGate, O = NoObserver> {
    epoch: u64,
    serial: u64,
    visibility: Visibility,
    gate: G,
    observer: O,
}
impl<G, O> Lifecycle<G, O> {
    pub const fn new(epoch: u64, gate: G, observer: O) -> Self {
        Self {
            epoch,
            serial: 0,
            visibility: Visibility::Metadata,
            gate,
            observer,
        }
    }
    pub const fn with_visibility(mut self, visibility: Visibility) -> Self {
        self.visibility = visibility;
        self
    }
    pub fn gate_mut(&mut self) -> &mut G {
        &mut self.gate
    }
    pub fn observer(&self) -> &O {
        &self.observer
    }
    pub fn observer_mut(&mut self) -> &mut O {
        &mut self.observer
    }
    pub const fn memory_bytes(&self) -> usize {
        core::mem::size_of::<Self>()
    }
    fn proposal<E>(&mut self, correlation: Correlation, event: E) -> Result<Proposal<E>, Fault> {
        let serial = self.serial.checked_add(1).ok_or(Error::Overflow)?;
        self.serial = serial;
        Ok(Proposal {
            serial,
            correlation,
            event,
        })
    }
    fn emit<E: Event>(&mut self, proposal: Proposal<E>, phase: Phase)
    where
        O: Observer<E>,
    {
        let event = if self.visibility == Visibility::Metadata {
            proposal.event.redacted()
        } else {
            proposal.event
        };
        self.observer.observe(Notice {
            proposal: Proposal { event, ..proposal },
            phase,
        });
    }
    pub fn begin<E: Event>(
        &mut self,
        correlation: Correlation,
        event: E,
    ) -> Result<HookResult<Span<E>>, Fault>
    where
        G: Gate<E>,
        O: Observer<E>,
    {
        self.begin_intercepted(correlation, event, &mut NoInterceptor)
    }
    /// Interception/validation happens before gating. Pause/deny invokes no action;
    /// the caller's original Copy candidate remains unchanged on every path.
    pub fn begin_intercepted<E: Event>(
        &mut self,
        correlation: Correlation,
        event: E,
        interceptor: &mut impl Interceptor<E>,
    ) -> Result<HookResult<Span<E>>, Fault>
    where
        G: Gate<E>,
        O: Observer<E>,
    {
        let mut proposal = self.proposal(correlation, event)?;
        if let Err(error) = interceptor
            .intercept(&mut proposal.event)
            .and_then(|()| proposal.event.validate())
        {
            self.emit(proposal, Phase::Rejected(error));
            return Err(error);
        }
        match self.gate.before(proposal) {
            Decision::Pause => {
                self.emit(proposal, Phase::Paused);
                Ok(HookResult::Paused)
            }
            Decision::Deny => {
                self.emit(proposal, Phase::Denied);
                Err(Error::Denied.into())
            }
            Decision::Allow => {
                self.emit(proposal, Phase::Started);
                Ok(HookResult::Applied(Span {
                    epoch: self.epoch,
                    proposal,
                    finished: false,
                }))
            }
        }
    }
    /// A terminal observation can happen once per receipt. Wrong-epoch/duplicate
    /// attempts preserve the receipt and emit nothing; caller can correct routing.
    pub fn finish<E: Event>(&mut self, span: &mut Span<E>, outcome: Outcome) -> Result<(), Fault>
    where
        O: Observer<E>,
    {
        if span.epoch != self.epoch || span.finished {
            return Err(Error::Stale.into());
        }
        span.finished = true;
        self.emit(span.proposal, Phase::Finished(outcome));
        Ok(())
    }
    /// Wrap a real operation, not just an event declaration. A returned adapter
    /// failure is observed and propagated unchanged, never retried or swallowed.
    pub fn run<E: Event, R>(
        &mut self,
        correlation: Correlation,
        event: E,
        operation: impl FnOnce(E) -> Result<R, Fault>,
    ) -> Result<HookResult<R>, Fault>
    where
        G: Gate<E>,
        O: Observer<E>,
    {
        self.run_intercepted(correlation, event, &mut NoInterceptor, operation)
    }
    pub fn run_intercepted<E: Event, R>(
        &mut self,
        correlation: Correlation,
        event: E,
        interceptor: &mut impl Interceptor<E>,
        operation: impl FnOnce(E) -> Result<R, Fault>,
    ) -> Result<HookResult<R>, Fault>
    where
        G: Gate<E>,
        O: Observer<E>,
    {
        let HookResult::Applied(mut span) =
            self.begin_intercepted(correlation, event, interceptor)?
        else {
            return Ok(HookResult::Paused);
        };
        let result = operation(span.proposal.event);
        self.finish(
            &mut span,
            match &result {
                Ok(_) => Outcome::Succeeded,
                Err(error) => Outcome::Failed(*error),
            },
        )?;
        result.map(HookResult::Applied)
    }
    /// Report an externally observed fact (timeout, usage, disconnect, etc.). It
    /// cannot be vetoed after it happened. This does not implement timers or I/O.
    pub fn notify<E: Event>(&mut self, correlation: Correlation, event: E) -> Result<(), Fault>
    where
        O: Observer<E>,
    {
        event.validate()?;
        let proposal = self.proposal(correlation, event)?;
        self.emit(proposal, Phase::Observed);
        Ok(())
    }
}
