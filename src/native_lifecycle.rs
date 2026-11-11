//! Hooked variants of native context operations, using typed lifecycle channels.
use crate::{context::{Budget, Change, Context, Group}, hooks::HookResult, lifecycle::{Correlation, Fault, Gate, Lifecycle, Observer, domains::{context, request}}, zcall::v1::{Format, Presence, RecordStorage, Request, Text, ToolDefinition}};

impl<'a, 's> Context<'a, 's> {
    pub fn build_with_lifecycle<'r, const N: usize, G, O>(&self, correlation: Correlation, format: Format, model: Presence<Text<'a>>, storage: &'r mut RecordStorage<'a, N>, tools: &[ToolDefinition<'a>], lifecycle: &mut Lifecycle<G, O>) -> Result<HookResult<Request<'a, 'r>>, Fault>
    where G: Gate<request::Event<'static>>, O: Observer<request::Event<'static>> {
        let event = request::Event::new(request::Action::Build).with_amount(self.tokens());
        lifecycle.run(correlation, event, |_| self.build(format, model, storage, tools).map_err(Fault::from))
    }
    pub fn set_budget_with_lifecycle<G, O>(&mut self, correlation: Correlation, budget: Budget, lifecycle: &mut Lifecycle<G, O>) -> Result<HookResult<()>, Fault>
    where G: Gate<context::Event<'static>>, O: Observer<context::Event<'static>> {
        let event = context::Event::new(context::Action::BudgetChange).with_amount(budget.limit);
        lifecycle.run(correlation, event, |_| self.set_budget(budget).map_err(Fault::from))
    }
    pub fn fork_with_lifecycle<'n, G, O>(&self, correlation: Correlation, storage: &'n mut [Option<Group<'a>>], lifecycle: &mut Lifecycle<G, O>) -> Result<HookResult<Context<'a, 'n>>, Fault>
    where G: Gate<context::Event<'static>>, O: Observer<context::Event<'static>> {
        let event = context::Event::new(context::Action::Fork).with_amount(self.tokens());
        lifecycle.run(correlation, event, |_| self.fork_into(storage).map_err(Fault::from))
    }
    pub fn compact_with_lifecycle<G, O>(&mut self, correlation: Correlation, range: core::ops::Range<usize>, summary: Group<'a>, lifecycle: &mut Lifecycle<G, O>) -> Result<HookResult<Change>, Fault>
    where G: Gate<context::Event<'static>>, O: Observer<context::Event<'static>> {
        let event = context::Event::new(context::Action::Compact).with_amount(summary.tokens);
        lifecycle.run(correlation, event, |_| self.compact(range, summary).map_err(Fault::from))
    }
}
