//! A bounded, in-memory host using the actual guarded tool execution API.
use zharness::{
    Error, Executor, Invocation, Tool, ToolOutput, ToolSet,
    guard::Authorization,
    validation::{ArgumentLimits, Arguments},
    zcall::v1::TokenStorage,
    zguard::{
        ActionKind, Audit, AuditEvent, AuditFailure, Context, Decision, Guard, Limits, Policy,
        Request, Rule,
    },
};

#[derive(Default)]
struct MemoryTool {
    calls: u64,
}
impl Executor for MemoryTool {
    fn execute(
        &mut self,
        _name: &str,
        _arguments: Arguments<'_>,
        output: &mut [u8],
    ) -> Result<ToolOutput, Error> {
        self.calls += 1;
        let result = b"accepted";
        output
            .get_mut(..result.len())
            .ok_or(Error::Capacity {
                needed: result.len(),
            })?
            .copy_from_slice(result);
        Ok(ToolOutput::success(result.len()))
    }
}
struct AuditCounter {
    decisions: usize,
}
impl Audit for AuditCounter {
    fn record(&mut self, event: AuditEvent) -> Result<(), AuditFailure> {
        self.decisions += 1;
        println!(
            "policy={} sequence={} decision={:?} reason={:?}",
            event.policy_id, event.sequence, event.verdict.decision, event.verdict.reason
        );
        Ok(())
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tools = [Tool::new(
        "notes.write",
        "Write a note",
        r#"{"type":"object"}"#,
    )];
    let registry = ToolSet::new(&tools)?;
    let rules = [Rule::new(
        1,
        ActionKind::Tool,
        "notes.write",
        Decision::RequireApproval,
    )];
    let mut guard = Guard::new(
        Policy::new(1, &rules)?,
        Context {
            session: 1,
            capabilities: 0,
        },
        Limits {
            max_calls: 4,
            max_payload_bytes: 256,
        },
    );
    let invocation = Invocation {
        call_id: "call-1",
        name: "notes.write",
        arguments: br#"{"note":"hello","approved":true}"#,
        signature: None,
    };
    let call = invocation.prepare(
        &registry,
        &mut TokenStorage::<64>::new(),
        ArgumentLimits::default(),
    )?;
    let mut adapter = MemoryTool::default();
    let mut audit = AuditCounter { decisions: 0 };
    let mut output = [0; 64];
    let error = call
        .execute_guarded(
            &mut adapter,
            &mut output,
            Authorization::new(&mut guard, &mut audit, 1),
        )
        .unwrap_err();
    assert!(matches!(error, Error::Guard(v) if v.decision == Decision::RequireApproval));
    assert_eq!(adapter.calls, 0);

    // This example host explicitly approves this exact in-memory request.
    // A real host obtains its decision through its authenticated operator channel.
    let approval = guard
        .approve(
            Request::tool(call.call_id(), call.name(), call.arguments().as_bytes()),
            10,
        )
        .map_err(Error::Guard)?;
    let result = call.execute_guarded(
        &mut adapter,
        &mut output,
        Authorization::new(&mut guard, &mut audit, 2).with_approval(approval),
    )?;
    assert_eq!(result.call_id, "call-1");
    assert_eq!(result.text, "accepted");
    assert_eq!(adapter.calls, 1);
    assert_eq!(audit.decisions, 2);
    println!(
        "executed={} remaining_calls={}",
        adapter.calls,
        guard.remaining_calls()
    );
    Ok(())
}
