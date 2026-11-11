//! Proposed/committed model configuration with native lifecycle gates.
//! Configuration affects future application requests only. Existing in-flight
//! model calls are not rewritten or cancelled; no provider capability is assumed.
use crate::{Error, hooks::HookResult, lifecycle::{Correlation, Fault, Gate, Interceptor, Lifecycle, NoInterceptor, Observer, Payload, domains::model::{Action, Configuration, Event, Reasoning}}};

pub struct ModelSettings<'a> { correlation: Correlation, current: Configuration<'a>, revision: u64 }
struct Target<'i, I>(&'i mut I);
impl<'a, I: Interceptor<Configuration<'a>>> Interceptor<Event<'a>> for Target<'_, I> {
    fn intercept(&mut self, event: &mut Event<'a>) -> Result<(), Fault> { self.0.intercept(&mut event.proposed) }
}
impl<'a> ModelSettings<'a> {
    pub fn select<G, O>(correlation: Correlation, initial: Configuration<'a>, lifecycle: &mut Lifecycle<G, O>) -> Result<HookResult<Self>, Fault>
    where G: Gate<Event<'a>>, O: Observer<Event<'a>> {
        let event = Event { action: Action::Select, previous: None, proposed: initial, revision: 0 };
        lifecycle.run(correlation, event, |event| Ok(Self { correlation, current: event.proposed, revision: 0 }))
    }
    pub fn current(&self) -> Configuration<'a> { self.current }
    pub fn revision(&self) -> u64 { self.revision }
    fn apply<G, O, I>(&mut self, action: Action, proposed: Configuration<'a>, filter: &mut I, lifecycle: &mut Lifecycle<G, O>) -> Result<HookResult<()>, Fault>
    where G: Gate<Event<'a>>, O: Observer<Event<'a>>, I: Interceptor<Configuration<'a>> {
        let revision = self.revision.checked_add(1).ok_or(Error::Overflow)?;
        let event = Event { action, previous: Some(self.current), proposed, revision: self.revision };
        lifecycle.run_intercepted(self.correlation, event, &mut Target(filter), |event| {
            self.current = event.proposed; self.revision = revision; Ok(())
        })
    }
    pub fn switch_model<G, O>(&mut self, model: &'a str, lifecycle: &mut Lifecycle<G, O>) -> Result<HookResult<()>, Fault>
    where G: Gate<Event<'a>>, O: Observer<Event<'a>> {
        self.apply(Action::Switch, Configuration { model, ..self.current }, &mut NoInterceptor, lifecycle)
    }
    pub fn set_reasoning<G, O>(&mut self, reasoning: Reasoning<'a>, lifecycle: &mut Lifecycle<G, O>) -> Result<HookResult<()>, Fault>
    where G: Gate<Event<'a>>, O: Observer<Event<'a>> {
        self.apply(Action::ReasoningChange, Configuration { reasoning, ..self.current }, &mut NoInterceptor, lifecycle)
    }
    pub fn set_route<G, O>(&mut self, route: Option<&'a str>, lifecycle: &mut Lifecycle<G, O>) -> Result<HookResult<()>, Fault>
    where G: Gate<Event<'a>>, O: Observer<Event<'a>> {
        self.apply(Action::RouteSelect, Configuration { route, ..self.current }, &mut NoInterceptor, lifecycle)
    }
    pub fn set_options<G, O>(&mut self, options: Payload<'a>, lifecycle: &mut Lifecycle<G, O>) -> Result<HookResult<()>, Fault>
    where G: Gate<Event<'a>>, O: Observer<Event<'a>> {
        self.apply(Action::OptionsChange, Configuration { options, ..self.current }, &mut NoInterceptor, lifecycle)
    }
    /// Filter a copied configuration, validate and gate the actual final value.
    /// The true old value, revision and correlation cannot be changed by a filter.
    pub fn configure<G, O, I>(&mut self, proposed: Configuration<'a>, filter: &mut I, lifecycle: &mut Lifecycle<G, O>) -> Result<HookResult<()>, Fault>
    where G: Gate<Event<'a>>, O: Observer<Event<'a>>, I: Interceptor<Configuration<'a>> {
        self.apply(Action::OptionsChange, proposed, filter, lifecycle)
    }
}
