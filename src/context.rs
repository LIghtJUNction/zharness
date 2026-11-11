//! Bounded, borrowed conversation groups with explicit token accounting.
//!
//! A group is the smallest eviction/compaction unit. Put an assistant's parallel
//! tool calls and all their results in one group. Counts come from the caller's
//! tokenizer or estimator; this module never treats character counts as tokens.
use core::ops::Range;
use crate::{Error, bridge::Completion, hooks::{self, After, Before, HookResult, Hooks}, zcall::v1::{Content, Format, Presence, RecordStorage, Request, RequestBuilder, Role, Text, ToolDefinition}};

#[derive(Clone, Copy, Debug)]
pub enum Message<'a> {
    Blocks { role: Role, content: &'a [Content<'a>] },
    /// Appended using zcall's ID-checked result API, not provider-specific JSON.
    ToolResult(Completion<'a, 'a>),
}
impl<'a> Message<'a> {
    pub const fn new(role: Role, content: &'a [Content<'a>]) -> Self { Self::Blocks { role, content } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Retention { Recent, Pinned }

#[derive(Clone, Copy, Debug)]
pub struct Group<'a> {
    pub id: u64,
    pub messages: &'a [Message<'a>],
    /// Caller-supplied cost, including message framing for the chosen model.
    pub tokens: u64,
    pub retention: Retention,
}
impl<'a> Group<'a> {
    pub const fn new(id: u64, messages: &'a [Message<'a>], tokens: u64) -> Self {
        Self { id, messages, tokens, retention: Retention::Recent }
    }
    pub const fn pinned(mut self) -> Self { self.retention = Retention::Pinned; self }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Budget {
    pub limit: u64,
    /// Reserve for generated output and non-history input such as tool schemas.
    pub reserved: u64,
}
impl Budget {
    pub fn available(self) -> Result<u64, Error> { self.limit.checked_sub(self.reserved).ok_or(Error::InvalidBudget) }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Overflow { Reject, EvictOldest }
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Change {
    pub removed_groups: usize,
    pub removed_tokens: u64,
    pub retained_tokens: u64,
}

pub struct Context<'a, 's> {
    storage: &'s mut [Option<Group<'a>>],
    len: usize,
    tokens: u64,
    budget: Budget,
}
impl<'a, 's> Context<'a, 's> {
    /// Clears descriptor slots, not the borrowed message contents.
    pub fn new(storage: &'s mut [Option<Group<'a>>], budget: Budget) -> Result<Self, Error> {
        budget.available()?;
        storage.fill(None);
        Ok(Self { storage, len: 0, tokens: 0, budget })
    }
    pub fn len(&self) -> usize { self.len }
    pub fn is_empty(&self) -> bool { self.len == 0 }
    pub fn tokens(&self) -> u64 { self.tokens }
    pub fn budget(&self) -> Budget { self.budget }
    pub fn groups(&self) -> impl DoubleEndedIterator<Item = Group<'a>> + '_ {
        self.storage[..self.len].iter().filter_map(|g| *g)
    }
    pub fn memory_bytes(&self) -> usize { core::mem::size_of::<Self>() + core::mem::size_of_val(self.storage) }
    pub fn set_budget(&mut self, budget: Budget) -> Result<(), Error> {
        let available = budget.available()?;
        if self.tokens > available { return Err(Error::BudgetExceeded { needed: self.tokens, available }); }
        self.budget = budget;
        Ok(())
    }
    fn check_group(&self, group: Group<'_>) -> Result<(), Error> {
        if group.messages.is_empty() { return Err(Error::InvalidArguments { offset: 0 }); }
        let available = self.budget.available()?;
        if group.tokens > available { return Err(Error::BudgetExceeded { needed: group.tokens, available }); }
        Ok(())
    }
    fn plan_push(&self, group: Group<'_>, overflow: Overflow) -> Result<usize, Error> {
        self.check_group(group)?;
        if self.groups().any(|g| g.id == group.id) { return Err(Error::DuplicateId); }
        let available = self.budget.available()?;
        let allowance = available - group.tokens;
        let mut tokens = self.tokens;
        let mut count = self.len;
        if count < self.storage.len() && tokens <= allowance { return Ok(0); }
        if overflow == Overflow::EvictOldest {
            for (at, old) in self.groups().enumerate() {
                if old.retention == Retention::Recent { tokens -= old.tokens; count -= 1; }
                if count < self.storage.len() && tokens <= allowance { return Ok(at + 1); }
            }
        }
        if count >= self.storage.len() { return Err(Error::Capacity { needed: count.saturating_add(1) }); }
        Err(Error::BudgetExceeded { needed: tokens.saturating_add(group.tokens), available })
    }
    fn apply_push(&mut self, group: Group<'a>, cutoff: usize) -> Change {
        let old_tokens = self.tokens;
        let old_len = self.len;
        let mut write = 0;
        for read in 0..old_len {
            let old = self.storage[read];
            if read < cutoff && old.is_some_and(|g| g.retention == Retention::Recent) {
                if let Some(old) = old { self.tokens -= old.tokens; }
            } else {
                self.storage[write] = old;
                write += 1;
            }
        }
        let removed_tokens = old_tokens - self.tokens;
        self.storage[write..old_len].fill(None);
        self.storage[write] = Some(group);
        self.len = write + 1;
        self.tokens += group.tokens;
        Change { removed_groups: old_len - write, removed_tokens, retained_tokens: self.tokens }
    }
    /// Failure leaves all groups, costs and ordering unchanged.
    pub fn push(&mut self, group: Group<'a>, overflow: Overflow) -> Result<Change, Error> {
        let cutoff = self.plan_push(group, overflow)?;
        Ok(self.apply_push(group, cutoff))
    }
    pub fn push_with_hooks(&mut self, group: Group<'a>, overflow: Overflow, hooks: &mut impl Hooks) -> Result<HookResult<Change>, Error> {
        let cutoff = self.plan_push(group, overflow)?;
        if !hooks::permit(hooks, Before::Context { group, compacting: false })? { return Ok(HookResult::Paused); }
        let change = self.apply_push(group, cutoff);
        hooks.after(After::ContextChanged { groups: self.len, tokens: self.tokens });
        Ok(HookResult::Applied(change))
    }
    fn plan_compaction(&self, range: &Range<usize>, summary: Group<'_>) -> Result<u64, Error> {
        self.check_group(summary)?;
        if range.start >= range.end || range.end > self.len { return Err(Error::InvalidRange); }
        let mut removed = 0;
        for (at, group) in self.groups().enumerate() {
            if range.contains(&at) {
                if group.retention == Retention::Pinned { return Err(Error::Pinned); }
                removed += group.tokens;
            } else if group.id == summary.id { return Err(Error::DuplicateId); }
        }
        let available = self.budget.available()?;
        if self.tokens - removed > available - summary.tokens {
            return Err(Error::BudgetExceeded { needed: (self.tokens - removed).saturating_add(summary.tokens), available });
        }
        Ok(removed)
    }
    fn apply_compaction(&mut self, range: Range<usize>, summary: Group<'a>, removed: u64) -> Change {
        let count = range.end - range.start;
        self.storage.copy_within(range.end..self.len, range.start + 1);
        self.storage[range.start] = Some(summary);
        let new_len = self.len - count + 1;
        self.storage[new_len..self.len].fill(None);
        self.len = new_len;
        self.tokens = self.tokens - removed + summary.tokens;
        Change { removed_groups: count, removed_tokens: removed, retained_tokens: self.tokens }
    }
    /// Replace whole unpinned groups with an application-produced summary.
    /// The library does not invoke a summarizer or silently rewrite instructions.
    pub fn compact(&mut self, range: Range<usize>, summary: Group<'a>) -> Result<Change, Error> {
        let removed = self.plan_compaction(&range, summary)?;
        Ok(self.apply_compaction(range, summary, removed))
    }
    pub fn compact_with_hooks(&mut self, range: Range<usize>, summary: Group<'a>, hooks: &mut impl Hooks) -> Result<HookResult<Change>, Error> {
        let removed = self.plan_compaction(&range, summary)?;
        if !hooks::permit(hooks, Before::Context { group: summary, compacting: true })? { return Ok(HookResult::Paused); }
        let change = self.apply_compaction(range, summary, removed);
        hooks.after(After::ContextChanged { groups: self.len, tokens: self.tokens });
        Ok(HookResult::Applied(change))
    }
    /// Independent descriptor storage, shared immutable content; no hidden deep copy.
    pub fn fork_into<'n>(&self, storage: &'n mut [Option<Group<'a>>]) -> Result<Context<'a, 'n>, Error> {
        if storage.len() < self.len { return Err(Error::Capacity { needed: self.len }); }
        storage.fill(None);
        storage[..self.len].copy_from_slice(&self.storage[..self.len]);
        Ok(Context { storage, len: self.len, tokens: self.tokens, budget: self.budget })
    }
    /// Build through zcall public APIs. No partial Request escapes on error.
    /// Tool-result IDs and duplicate results are validated by zcall.
    pub fn build<'r, const N: usize>(&self, format: Format, model: Presence<Text<'a>>, storage: &'r mut RecordStorage<'a, N>, tools: &[ToolDefinition<'a>]) -> Result<Request<'a, 'r>, Error> {
        let mut builder = RequestBuilder::new(format, model, storage)?;
        for tool in tools { builder.tool(*tool)?; }
        let mut request = builder.finish();
        for group in self.groups() {
            for message in group.messages {
                match *message {
                    Message::Blocks { role, content } => request.append_message(role, content)?,
                    Message::ToolResult(result) => result.append_to(&mut request)?,
                }
            }
        }
        Ok(request)
    }
}
