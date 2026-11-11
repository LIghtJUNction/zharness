use zharness::{Error, bridge::Completion, context::{Budget, Context, Group, Message, Overflow}, hooks::{After, Before, Decision, HookResult, Hooks}, zcall::v1::*};

static TEXT: [Content<'static>; 1] = [Content::text("history")];
static MESSAGES: [Message<'static>; 1] = [Message::new(Role::User, &TEXT)];
fn group(id: u64, tokens: u64) -> Group<'static> { Group::new(id, &MESSAGES, tokens) }
fn ids(context: &Context<'_, '_>) -> Vec<u64> { context.groups().map(|g| g.id).collect() }

#[test]
fn evicts_complete_groups_and_keeps_pins() {
    let mut slots = [None; 3];
    let mut c = Context::new(&mut slots, Budget { limit: 20, reserved: 5 }).unwrap();
    c.push(group(0, 5).pinned(), Overflow::Reject).unwrap();
    c.push(group(1, 5), Overflow::Reject).unwrap();
    c.push(group(2, 5), Overflow::Reject).unwrap();
    let change = c.push(group(3, 9), Overflow::EvictOldest).unwrap();
    assert_eq!(ids(&c), [0, 3]); assert_eq!(c.tokens(), 14);
    assert_eq!(change.removed_groups, 2); assert_eq!(change.removed_tokens, 10);
}
#[test]
fn budget_capacity_and_overflow_fail_atomically() {
    let mut slots = [None; 1];
    let mut c = Context::new(&mut slots, Budget { limit: u64::MAX, reserved: 1 }).unwrap();
    c.push(group(1, u64::MAX - 1).pinned(), Overflow::Reject).unwrap();
    assert!(c.push(group(2, 1), Overflow::EvictOldest).is_err());
    assert!(c.push(group(2, u64::MAX), Overflow::EvictOldest).is_err());
    assert_eq!(ids(&c), [1]); assert_eq!(c.tokens(), u64::MAX - 1);
    assert!(c.set_budget(Budget { limit: 1, reserved: 2 }).is_err());
    assert!(c.set_budget(Budget { limit: 1, reserved: 0 }).is_err());
    let mut empty = []; let mut zero = Context::new(&mut empty, Budget { limit: 1, reserved: 0 }).unwrap();
    assert_eq!(zero.push(group(1, 1), Overflow::EvictOldest), Err(Error::Capacity { needed: 1 }));
}
#[test]
fn summaries_and_forks_are_explicit_and_bounded() {
    let mut slots = [None; 4]; let mut fork_slots = [None; 4];
    let mut c = Context::new(&mut slots, Budget { limit: 100, reserved: 10 }).unwrap();
    c.push(group(0, 10).pinned(), Overflow::Reject).unwrap();
    c.push(group(1, 20), Overflow::Reject).unwrap(); c.push(group(2, 20), Overflow::Reject).unwrap();
    assert_eq!(c.compact(0..2, group(9, 5)), Err(Error::Pinned));
    assert_eq!(c.compact(1..2, group(2, 5)), Err(Error::DuplicateId));
    assert_eq!(c.compact(2..5, group(9, 5)), Err(Error::InvalidRange));
    let mut fork = c.fork_into(&mut fork_slots).unwrap();
    fork.compact(1..3, group(9, 7)).unwrap();
    assert_eq!(ids(&fork), [0, 9]); assert_eq!(fork.tokens(), 17);
    assert_eq!(ids(&c), [0, 1, 2]); assert_eq!(c.tokens(), 50);
    assert!(c.fork_into(&mut []).is_err());
}
struct Gate { decision: Decision, observed: usize }
impl Hooks for Gate {
    fn before(&mut self, _: Before<'_>) -> Decision { self.decision }
    fn after(&mut self, _: After<'_>) { self.observed += 1; }
}
#[test]
fn paused_or_denied_hooks_do_not_mutate_context() {
    let mut slots = [None; 2]; let mut c = Context::new(&mut slots, Budget { limit: 10, reserved: 0 }).unwrap();
    let mut hooks = Gate { decision: Decision::Pause, observed: 0 };
    assert_eq!(c.push_with_hooks(group(1, 5), Overflow::Reject, &mut hooks).unwrap(), HookResult::Paused);
    assert!(c.is_empty()); assert_eq!(hooks.observed, 0);
    hooks.decision = Decision::Deny;
    assert_eq!(c.push_with_hooks(group(1, 5), Overflow::Reject, &mut hooks), Err(Error::Denied));
    hooks.decision = Decision::Allow;
    assert!(matches!(c.push_with_hooks(group(1, 5), Overflow::Reject, &mut hooks).unwrap(), HookResult::Applied(_)));
    assert_eq!(hooks.observed, 1);
}
#[test]
fn zcall_build_keeps_calls_and_results_in_one_group() {
    let calls = [Content::ToolCall { id: Presence::Value(Text::new("real-7")), name: Text::new("echo"), arguments: Data::EmptyObject, legacy: false }];
    let messages = [Message::new(Role::Assistant, &calls), Message::ToolResult(Completion { call_id: "real-7", text: "ok", is_error: false })];
    let mut slots = [None; 1]; let mut c = Context::new(&mut slots, Budget { limit: 100, reserved: 0 }).unwrap();
    c.push(Group::new(7, &messages, 10), Overflow::Reject).unwrap();
    let mut records = RecordStorage::<16>::new();
    let request = c.build(Protocol::OpenAiChat.into(), Presence::Value(Text::new("test")), &mut records, &[]).unwrap();
    assert_eq!(request.messages().count(), 2);
    let mut small = RecordStorage::<1>::new();
    assert!(c.build(Protocol::OpenAiChat.into(), Presence::Missing, &mut small, &[]).is_err());
    c.push(group(8, 1), Overflow::EvictOldest).unwrap();
    assert_eq!(ids(&c), [8]);
}
