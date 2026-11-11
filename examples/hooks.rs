use zharness::{Error, ToolLoop, hooks::{After, Before, Decision, Hooks}, turn::{Action, CallSlot, LoopLimits}};

#[derive(Default)]
struct Audit { proposed: usize, observed: usize }
impl Hooks for Audit {
    fn before(&mut self, _: Before<'_>) -> Decision { self.proposed += 1; Decision::Allow }
    fn after(&mut self, _: After<'_>) { self.observed += 1; }
}
struct Approval { approved: bool }
impl Hooks for Approval {
    fn before(&mut self, event: Before<'_>) -> Decision {
        if matches!(event, Before::Model { .. }) && !self.approved { Decision::Pause } else { Decision::Allow }
    }
}
fn main() -> Result<(), Error> {
    let mut slots = [CallSlot::<256>::new()];
    let mut flow = ToolLoop::new(42, &mut slots, LoopLimits::default());
    let mut hooks = (Audit::default(), Approval { approved: false });
    assert_eq!(flow.next(&mut hooks)?, Action::Paused);
    assert_eq!(flow.round(), 0);
    // A UI or policy service may supply this decision. The hook does not poll it.
    hooks.1.approved = true;
    assert_eq!(flow.next(&mut hooks)?, Action::CallModel { run: 42, round: 1 });
    // No network call has happened: the application still owns that action.
    flow.cancel(&mut hooks);
    assert_eq!(hooks.0.proposed, 2); assert_eq!(hooks.0.observed, 1);
    println!("Two proposals, one cancellation observation; no hidden model request.");
    Ok(())
}
