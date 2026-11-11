//! Application-driven multi-agent coordination: capabilities, DAG tasks, fenced
//! leases, handoff, join and bounded mailboxes. No worker threads or networking.
//!
//! Leases fence *board completions*, not external side effects. A caller must
//! use idempotency/fencing at its tool boundary before retrying expired work.
use crate::{Error, hooks::{self, After, Before, HookResult, Hooks}};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Agent { pub id: u64, pub capabilities: u64 }
#[derive(Clone, Copy, Debug)]
pub struct Task<'a> {
    pub id: u64,
    pub requirements: u64,
    pub payload: &'a [u8],
    /// Dependencies must already exist. This topological insertion rule forbids cycles.
    pub after: &'a [u64],
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TaskResult<'a> { pub output: &'a [u8], pub is_error: bool }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Lease { epoch: u64, task: u64, agent: u64, attempt: u64, expires: u64 }
impl Lease {
    pub fn epoch(self) -> u64 { self.epoch }
    pub fn task(self) -> u64 { self.task }
    pub fn agent(self) -> u64 { self.agent }
    pub fn attempt(self) -> u64 { self.attempt }
    pub fn expires(self) -> u64 { self.expires }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskState<'a> {
    Empty, Pending, Running(Lease), Completed(TaskResult<'a>), Cancelled,
    /// A dependency failed or was cancelled. No worker will be assigned.
    Blocked { dependency: u64 },
}
#[derive(Clone, Copy, Debug)]
pub struct TaskSlot<'a> { task: Option<Task<'a>>, state: TaskState<'a> }
impl TaskSlot<'_> { pub const EMPTY: Self = Self { task: None, state: TaskState::Empty }; }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub enum Assignment { Lease(Lease), Idle, Paused }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Join { Pending, Ready, Failed(u64), Cancelled(u64) }

pub struct TaskBoard<'a, 's> {
    agents: &'a [Agent],
    slots: &'s mut [TaskSlot<'a>],
    epoch: u64,
    attempt: u64,
    clock: u64,
}
impl<'a, 's> TaskBoard<'a, 's> {
    /// The application supplies a distinct epoch after board restart/recreation.
    /// Slots are cleared only after agent identities have been validated.
    pub fn new(epoch: u64, agents: &'a [Agent], slots: &'s mut [TaskSlot<'a>]) -> Result<Self, Error> {
        for (at, agent) in agents.iter().enumerate() {
            if agents[..at].iter().any(|old| old.id == agent.id) { return Err(Error::DuplicateId); }
        }
        slots.fill(TaskSlot::EMPTY);
        Ok(Self { agents, slots, epoch, attempt: 0, clock: 0 })
    }
    fn index(&self, id: u64) -> Result<usize, Error> {
        self.slots.iter().position(|s| s.task.is_some_and(|t| t.id == id)).ok_or(Error::UnknownId)
    }
    fn agent(&self, id: u64) -> Result<Agent, Error> {
        self.agents.iter().find(|a| a.id == id).copied().ok_or(Error::UnknownId)
    }
    fn check_time(&self, now: u64) -> Result<(), Error> {
        if now < self.clock { Err(Error::InvalidState) } else { Ok(()) }
    }
    fn deadline(now: u64, duration: u64) -> Result<u64, Error> {
        if duration == 0 { return Err(Error::InvalidState); }
        now.checked_add(duration).ok_or(Error::Overflow)
    }
    fn busy(&self, id: u64) -> bool {
        self.slots.iter().any(|s| matches!(s.state, TaskState::Running(lease) if lease.agent == id))
    }
    pub fn len(&self) -> usize { self.slots.iter().filter(|s| s.task.is_some()).count() }
    pub fn is_empty(&self) -> bool { self.len() == 0 }
    pub fn memory_bytes(&self) -> usize { core::mem::size_of::<Self>() + core::mem::size_of_val(self.slots) + core::mem::size_of_val(self.agents) }
    pub fn state(&self, task: u64) -> Result<TaskState<'a>, Error> { Ok(self.slots[self.index(task)?].state) }
    pub fn submit(&mut self, task: Task<'a>) -> Result<(), Error> {
        if self.index(task.id).is_ok() { return Err(Error::DuplicateId); }
        for (at, &dep) in task.after.iter().enumerate() {
            if dep == task.id { return Err(Error::InvalidState); }
            if task.after[..at].contains(&dep) { return Err(Error::DuplicateId); }
            self.index(dep)?;
        }
        let needed = self.len().saturating_add(1);
        let slot = self.slots.iter_mut().find(|s| s.task.is_none()).ok_or(Error::Capacity { needed })?;
        *slot = TaskSlot { task: Some(task), state: TaskState::Pending };
        self.propagate_blocked();
        Ok(())
    }
    /// Join a caller-selected set of tasks. Empty sets are ready.
    pub fn join(&self, tasks: &[u64]) -> Result<Join, Error> {
        let mut pending = false;
        for &id in tasks {
            match self.state(id)? {
                TaskState::Completed(result) if !result.is_error => {}
                TaskState::Completed(_) | TaskState::Blocked { .. } => return Ok(Join::Failed(id)),
                TaskState::Cancelled => return Ok(Join::Cancelled(id)),
                _ => pending = true,
            }
        }
        Ok(if pending { Join::Pending } else { Join::Ready })
    }
    fn propagate_blocked(&mut self) {
        // Bounded fixed-point pass handles recycled slots without recursion.
        for _ in 0..self.slots.len() {
            let mut changed = false;
            for at in 0..self.slots.len() {
                if self.slots[at].state != TaskState::Pending { continue; }
                let Some(task) = self.slots[at].task else { continue; };
                let blocked = task.after.iter().copied().find(|id| {
                    matches!(self.state(*id), Ok(TaskState::Completed(TaskResult { is_error: true, .. }) | TaskState::Cancelled | TaskState::Blocked { .. }))
                });
                if let Some(dependency) = blocked {
                    self.slots[at].state = TaskState::Blocked { dependency }; changed = true;
                }
            }
            if !changed { break; }
        }
    }
    /// Choose the first eligible task in slot order. One active lease per agent.
    /// Expired work is not retried implicitly: call expire deliberately first.
    pub fn claim(&mut self, agent: u64, now: u64, duration: u64, hooks: &mut impl Hooks) -> Result<Assignment, Error> {
        self.check_time(now)?;
        let expires = Self::deadline(now, duration)?;
        let agent = self.agent(agent)?;
        if self.busy(agent.id) { return Err(Error::Busy); }
        let selected = self.slots.iter().enumerate().find_map(|(at, slot)| {
            let task = slot.task?;
            (slot.state == TaskState::Pending && agent.capabilities & task.requirements == task.requirements && self.join(task.after) == Ok(Join::Ready)).then_some((at, task))
        });
        let Some((at, task)) = selected else { return Ok(Assignment::Idle); };
        let attempt = self.attempt.checked_add(1).ok_or(Error::Overflow)?;
        if !hooks::permit(hooks, Before::Assignment { task: task.id, agent: agent.id, attempt })? { return Ok(Assignment::Paused); }
        let lease = Lease { epoch: self.epoch, task: task.id, agent: agent.id, attempt, expires };
        self.slots[at].state = TaskState::Running(lease);
        self.attempt = attempt; self.clock = now;
        hooks.after(After::Assigned { task: task.id, agent: agent.id, attempt });
        Ok(Assignment::Lease(lease))
    }
    fn validate(&self, lease: Lease, now: u64) -> Result<usize, Error> {
        self.check_time(now)?;
        let at = self.index(lease.task).map_err(|_| Error::Stale)?;
        if lease.epoch != self.epoch || self.slots[at].state != TaskState::Running(lease) || now >= lease.expires { return Err(Error::Stale); }
        Ok(at)
    }
    pub fn task(&self, lease: Lease, now: u64) -> Result<Task<'a>, Error> {
        self.slots[self.validate(lease, now)?].task.ok_or(Error::Stale)
    }
    /// Completion is accepted once and only from the currently leased attempt.
    pub fn complete(&mut self, lease: Lease, now: u64, result: TaskResult<'a>, hooks: &mut impl Hooks) -> Result<(), Error> {
        let at = self.validate(lease, now)?;
        self.slots[at].state = TaskState::Completed(result); self.clock = now;
        if result.is_error { self.propagate_blocked(); }
        hooks.after(After::TaskFinished { task: lease.task, agent: lease.agent, attempt: lease.attempt, is_error: result.is_error });
        Ok(())
    }
    /// Renewing replaces the lease value; the old value is fenced out immediately.
    pub fn renew(&mut self, lease: Lease, now: u64, duration: u64) -> Result<Lease, Error> {
        let at = self.validate(lease, now)?;
        let expires = Self::deadline(now, duration)?;
        if expires <= lease.expires { return Err(Error::InvalidState); }
        let renewed = Lease { expires, ..lease };
        self.slots[at].state = TaskState::Running(renewed); self.clock = now;
        Ok(renewed)
    }
    /// Transfer to an idle compatible agent and invalidate the former worker's lease.
    pub fn handoff(&mut self, lease: Lease, agent: u64, now: u64, duration: u64, hooks: &mut impl Hooks) -> Result<HookResult<Lease>, Error> {
        let at = self.validate(lease, now)?;
        let expires = Self::deadline(now, duration)?;
        let target = self.agent(agent)?;
        if self.busy(agent) { return Err(Error::Busy); }
        let task = self.slots[at].task.ok_or(Error::Stale)?;
        if target.capabilities & task.requirements != task.requirements { return Err(Error::Incompatible); }
        let attempt = self.attempt.checked_add(1).ok_or(Error::Overflow)?;
        if !hooks::permit(hooks, Before::Assignment { task: task.id, agent, attempt })? { return Ok(HookResult::Paused); }
        let new = Lease { epoch: self.epoch, task: task.id, agent, attempt, expires };
        self.slots[at].state = TaskState::Running(new); self.attempt = attempt; self.clock = now;
        hooks.after(After::Assigned { task: task.id, agent, attempt });
        Ok(HookResult::Applied(new))
    }
    /// Explicitly requeue expired leases. External work is NOT stopped or undone.
    pub fn expire(&mut self, now: u64, hooks: &mut impl Hooks) -> Result<usize, Error> {
        self.check_time(now)?;
        let mut count = 0;
        for slot in self.slots.iter_mut() {
            if let TaskState::Running(lease) = slot.state && now >= lease.expires {
                slot.state = TaskState::Pending; count += 1;
                hooks.after(After::LeaseExpired { task: lease.task, agent: lease.agent, attempt: lease.attempt });
            }
        }
        self.clock = now;
        Ok(count)
    }
    pub fn cancel(&mut self, task: u64, hooks: &mut impl Hooks) -> Result<(), Error> {
        let at = self.index(task)?;
        if !matches!(self.slots[at].state, TaskState::Pending | TaskState::Running(_)) { return Err(Error::InvalidState); }
        self.slots[at].state = TaskState::Cancelled;
        self.propagate_blocked();
        hooks.after(After::TaskCancelled { task });
        Ok(())
    }
    /// Reclaim terminal descriptors only after every dependent has been retired.
    /// The attempt counter is not reset; reused task IDs cannot resurrect a lease.
    pub fn retire(&mut self, task: u64) -> Result<(), Error> {
        let at = self.index(task)?;
        if !matches!(self.slots[at].state, TaskState::Completed(_) | TaskState::Cancelled | TaskState::Blocked { .. }) { return Err(Error::Busy); }
        if self.slots.iter().any(|s| s.task.is_some_and(|t| t.after.contains(&task))) { return Err(Error::Busy); }
        self.slots[at] = TaskSlot::EMPTY;
        Ok(())
    }
}

/// Data is borrowed, not copied into an implicit network/shared-memory buffer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Envelope<'a> { pub from: u64, pub to: u64, pub correlation: u64, pub payload: &'a [u8] }

/// Bounded local mailbox. Not a lock-free queue, transport, or authorization layer.
/// The application serializes access (or wraps it in its chosen synchronization).
pub struct Mailbox<'s, T> { slots: &'s mut [Option<T>], len: usize }
impl<'s, T> Mailbox<'s, T> {
    /// Drops prior slot contents. No message allocation is performed by the mailbox.
    pub fn new(slots: &'s mut [Option<T>]) -> Self { slots.fill_with(|| None); Self { slots, len: 0 } }
    pub fn len(&self) -> usize { self.len }
    pub fn is_empty(&self) -> bool { self.len == 0 }
    pub fn capacity(&self) -> usize { self.slots.len() }
    pub fn memory_bytes(&self) -> usize { core::mem::size_of::<Self>() + core::mem::size_of_val(self.slots) }
    /// On capacity failure, ownership of the original message is returned intact.
    pub fn send(&mut self, message: T) -> Result<(), T> {
        if self.len == self.slots.len() { return Err(message); }
        self.slots[self.len] = Some(message); self.len += 1; Ok(())
    }
    pub fn receive(&mut self) -> Option<T> { self.receive_matching(|_| true) }
    /// Removes the first match and preserves the order of all other messages.
    pub fn receive_matching(&mut self, mut predicate: impl FnMut(&T) -> bool) -> Option<T> {
        let at = self.slots[..self.len].iter().position(|s| s.as_ref().is_some_and(&mut predicate))?;
        let message = self.slots[at].take();
        for i in at..self.len - 1 { self.slots[i] = self.slots[i + 1].take(); }
        self.len -= 1;
        message
    }
}
impl<'a> Mailbox<'_, Envelope<'a>> {
    pub fn receive_for(&mut self, agent: u64) -> Option<Envelope<'a>> { self.receive_matching(|message| message.to == agent) }
}
