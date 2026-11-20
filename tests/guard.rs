use std::{cell::Cell, rc::Rc};
use zharness::{
    Error, Executor, Invocation, Tool, ToolOutput, ToolSet,
    bridge::Prepared,
    command::{Command, CommandError, CommandExecutor, CommandOutput, ExitStatus},
    execution::ExecuteError,
    guard::{Authorization, GuardedCommandExecutor, GuardedExecutor},
    hooks::{Decision as HookDecision, HookResult, Hooks},
    plugin::{Route, Router, Slot},
    validation::{ArgumentLimits, Arguments},
    zcall::v1::TokenStorage,
    zguard::{
        ActionKind, Audit, AuditEvent, AuditFailure, Context, Decision, Guard, Limits, NoAudit,
        Policy, Reason, Request, Rule,
    },
};

fn prepared<'a>(id: &'a str, name: &'a str, bytes: &'a [u8]) -> Prepared<'a> {
    let tools = [Tool::new(name, "fixture", r#"{"type":"object"}"#)];
    Invocation {
        call_id: id,
        name,
        arguments: bytes,
        signature: None,
    }
    .prepare(
        &ToolSet::new(&tools).unwrap(),
        &mut TokenStorage::<64>::new(),
        ArgumentLimits::default(),
    )
    .unwrap()
}
fn guard<'a>(rules: &'a [Rule<'a>]) -> Guard<'a> {
    Guard::new(
        Policy::new(1, rules).unwrap(),
        Context {
            session: 7,
            capabilities: 0,
        },
        Limits::default(),
    )
}
#[derive(Default)]
struct Spy {
    calls: usize,
    fail: bool,
    business_error: bool,
    name: String,
    args: Vec<u8>,
    audited: Option<Rc<Cell<bool>>>,
}
impl Executor for Spy {
    fn execute(
        &mut self,
        name: &str,
        args: Arguments<'_>,
        output: &mut [u8],
    ) -> Result<ToolOutput, Error> {
        if let Some(audited) = &self.audited {
            assert!(audited.get());
        }
        self.calls += 1;
        self.name = name.to_string();
        self.args = args.as_bytes().to_vec();
        if self.fail {
            return Err(Error::ExecutionFailed);
        }
        output[..4].copy_from_slice(b"done");
        Ok(ToolOutput {
            written: 4,
            is_error: self.business_error,
        })
    }
}
#[derive(Default)]
struct AuditLog {
    events: Vec<AuditEvent>,
    fail: bool,
    observed: Option<Rc<Cell<bool>>>,
}
impl Audit for AuditLog {
    fn record(&mut self, event: AuditEvent) -> Result<(), AuditFailure> {
        self.events.push(event);
        if self.fail {
            return Err(AuditFailure);
        }
        if let Some(observed) = &self.observed {
            observed.set(true);
        }
        Ok(())
    }
}

#[test]
fn empty_policy_denies_before_adapter_and_preserves_output() {
    let mut guard = guard(&[]);
    let mut audit = AuditLog::default();
    let mut spy = Spy::default();
    let mut output = [0xA5; 16];
    let error = prepared("c1", "read", b"{}")
        .execute_guarded(
            &mut spy,
            &mut output,
            Authorization::new(&mut guard, &mut audit, 1),
        )
        .unwrap_err();
    assert!(matches!(error, Error::Guard(v) if v.reason == Reason::NoMatchingRule));
    assert_eq!(spy.calls, 0);
    assert_eq!(output, [0xA5; 16]);
    assert_eq!(guard.calls(), 0);
    assert_eq!(audit.events.len(), 1);
}

#[test]
fn native_allow_preserves_call_identity_arguments_and_business_error() {
    let rules = [Rule::new(1, ActionKind::Tool, "read", Decision::Allow)];
    let observed = Rc::new(Cell::new(false));
    let mut audit = AuditLog {
        observed: Some(observed.clone()),
        ..AuditLog::default()
    };
    let mut spy = Spy {
        business_error: true,
        audited: Some(observed),
        ..Spy::default()
    };
    let mut guard = guard(&rules);
    let mut output = [0; 16];
    let result = prepared("provider-call-id", "read", br#"{"path":"notes"}"#)
        .execute_guarded(
            &mut spy,
            &mut output,
            Authorization::new(&mut guard, &mut audit, 2),
        )
        .unwrap();
    assert_eq!(result.call_id, "provider-call-id");
    assert_eq!(result.text, "done");
    assert!(result.is_error);
    assert_eq!(spy.calls, 1);
    assert_eq!(spy.name, "read");
    assert_eq!(spy.args, br#"{"path":"notes"}"#);
    assert_eq!(guard.calls(), 1);
    assert_eq!(audit.events.len(), 1);
}

#[test]
fn model_approval_fields_do_not_grant_host_permission() {
    let rules = [Rule::new(
        1,
        ActionKind::Tool,
        "write",
        Decision::RequireApproval,
    )];
    let mut guard = guard(&rules);
    let mut spy = Spy::default();
    let mut output = [0; 16];
    let call = prepared(
        "c1",
        "write",
        br#"{"approved":true,"capabilities":18446744073709551615}"#,
    );
    let error = call
        .execute_guarded(
            &mut spy,
            &mut output,
            Authorization::new(&mut guard, &mut NoAudit, 1),
        )
        .unwrap_err();
    assert!(matches!(error, Error::Guard(v) if v.decision == Decision::RequireApproval));
    assert_eq!(spy.calls, 0);

    let approval = guard
        .approve(
            Request::tool(call.call_id(), call.name(), call.arguments().as_bytes()),
            10,
        )
        .unwrap();
    call.execute_guarded(
        &mut spy,
        &mut output,
        Authorization::new(&mut guard, &mut NoAudit, 2).with_approval(approval),
    )
    .unwrap();
    assert_eq!(spy.calls, 1);
    let error = call
        .execute_guarded(
            &mut spy,
            &mut output,
            Authorization::new(&mut guard, &mut NoAudit, 3),
        )
        .unwrap_err();
    assert!(matches!(error, Error::Guard(v) if v.reason == Reason::ApprovalRequired));
    assert_eq!(spy.calls, 1);
}

#[test]
fn prepared_approval_binds_actual_id_tool_and_exact_json_bytes() {
    let rules = [
        Rule::new(1, ActionKind::Tool, "write", Decision::RequireApproval),
        Rule::new(2, ActionKind::Tool, "other", Decision::RequireApproval),
    ];
    let mut guard = guard(&rules);
    let mut spy = Spy::default();
    let mut output = [7; 16];
    for call in [
        prepared("changed", "write", b"{}"),
        prepared("c1", "other", b"{}"),
        prepared("c1", "write", b"{ }"),
    ] {
        let approval = guard
            .approve(Request::tool("c1", "write", b"{}"), 10)
            .unwrap();
        let error = call
            .execute_guarded(
                &mut spy,
                &mut output,
                Authorization::new(&mut guard, &mut NoAudit, 1).with_approval(approval),
            )
            .unwrap_err();
        assert!(matches!(error, Error::Guard(v) if v.reason == Reason::ApprovalMismatch));
    }
    assert_eq!(spy.calls, 0);
    assert_eq!(output, [7; 16]);
}

#[test]
fn expired_approval_never_invokes_adapter() {
    let rules = [Rule::new(
        1,
        ActionKind::Tool,
        "write",
        Decision::RequireApproval,
    )];
    let mut guard = guard(&rules);
    let approval = guard
        .approve(Request::tool("c1", "write", b"{}"), 5)
        .unwrap();
    let mut spy = Spy::default();
    let error = prepared("c1", "write", b"{}")
        .execute_guarded(
            &mut spy,
            &mut [0; 16],
            Authorization::new(&mut guard, &mut NoAudit, 5).with_approval(approval),
        )
        .unwrap_err();
    assert!(matches!(error, Error::Guard(v) if v.reason == Reason::ApprovalExpired));
    assert_eq!(spy.calls, 0);
}

#[test]
fn audit_failure_blocks_effects_and_metadata_has_no_request_content() {
    let rules = [Rule::new(
        1,
        ActionKind::Tool,
        "private.tool",
        Decision::Allow,
    )];
    let mut guard = guard(&rules);
    let mut audit = AuditLog {
        fail: true,
        ..AuditLog::default()
    };
    let mut spy = Spy::default();
    let mut output = [9; 16];
    let error = prepared(
        "sensitive-call-id",
        "private.tool",
        br#"{"token":"secret-value"}"#,
    )
    .execute_guarded(
        &mut spy,
        &mut output,
        Authorization::new(&mut guard, &mut audit, 1),
    )
    .unwrap_err();
    assert!(matches!(error, Error::Guard(v) if v.reason == Reason::AuditFailed));
    assert_eq!(spy.calls, 0);
    assert_eq!(output, [9; 16]);
    let recorded = format!("{:?}", audit.events);
    for secret in ["secret-value", "token", "private.tool", "sensitive-call-id"] {
        assert!(!recorded.contains(secret));
    }
}

#[test]
fn adapter_failures_do_not_refund_call_budget() {
    let rules = [Rule::new(1, ActionKind::Tool, "write", Decision::Allow)];
    let mut guard = Guard::new(
        Policy::new(1, &rules).unwrap(),
        Context {
            session: 7,
            capabilities: 0,
        },
        Limits {
            max_calls: 1,
            max_payload_bytes: 128,
        },
    );
    let mut spy = Spy {
        fail: true,
        ..Spy::default()
    };
    let call = prepared("c1", "write", b"{}");
    assert!(matches!(
        call.execute_guarded(
            &mut spy,
            &mut [0; 16],
            Authorization::new(&mut guard, &mut NoAudit, 1)
        ),
        Err(Error::ExecutionFailed)
    ));
    let error = call
        .execute_guarded(
            &mut spy,
            &mut [0; 16],
            Authorization::new(&mut guard, &mut NoAudit, 2),
        )
        .unwrap_err();
    assert!(matches!(error, Error::Guard(v) if v.reason == Reason::CallLimit));
    assert_eq!(spy.calls, 1);
    assert_eq!(guard.calls(), 1);
}

#[test]
fn payload_limit_is_checked_at_actual_execution() {
    let rules = [Rule::new(1, ActionKind::Tool, "read", Decision::Allow)];
    let mut guard = Guard::new(
        Policy::new(1, &rules).unwrap(),
        Context {
            session: 7,
            capabilities: 0,
        },
        Limits {
            max_calls: 2,
            max_payload_bytes: 2,
        },
    );
    let mut spy = Spy::default();
    prepared("c1", "read", b"{}")
        .execute_guarded(
            &mut spy,
            &mut [0; 16],
            Authorization::new(&mut guard, &mut NoAudit, 1),
        )
        .unwrap();
    let error = prepared("c2", "read", b"{ }")
        .execute_guarded(
            &mut spy,
            &mut [0; 16],
            Authorization::new(&mut guard, &mut NoAudit, 2),
        )
        .unwrap_err();
    assert!(matches!(error, Error::Guard(v) if v.reason == Reason::PayloadLimit));
    assert_eq!(spy.calls, 1);
}

struct TestHooks {
    decision: HookDecision,
    errors: usize,
}
impl Hooks for TestHooks {
    fn before_execute(&mut self, _: zharness::execution::BeforeExecute<'_>) -> HookDecision {
        self.decision
    }
    fn execute_error(&mut self, _: ExecuteError<'_>) {
        self.errors += 1;
    }
}
#[test]
fn hook_pause_precedes_guard_and_guard_rejections_reach_error_observer() {
    let mut guard = guard(&[]);
    let mut audit = AuditLog::default();
    let mut spy = Spy::default();
    let mut hooks = TestHooks {
        decision: HookDecision::Pause,
        errors: 0,
    };
    assert!(matches!(
        prepared("c1", "read", b"{}").execute_guarded_with_hooks(
            &mut spy,
            &mut [0; 16],
            Authorization::new(&mut guard, &mut audit, 1),
            &mut hooks
        ),
        Ok(HookResult::Paused)
    ));
    assert!(audit.events.is_empty());
    assert_eq!(guard.calls(), 0);
    hooks.decision = HookDecision::Allow;
    assert!(matches!(
        prepared("c1", "read", b"{}").execute_guarded_with_hooks(
            &mut spy,
            &mut [0; 16],
            Authorization::new(&mut guard, &mut audit, 2),
            &mut hooks
        ),
        Err(Error::Guard(_))
    ));
    assert_eq!(hooks.errors, 1);
    assert_eq!(spy.calls, 0);
}

#[test]
fn existing_executor_trait_calls_can_use_guarded_wrapper() {
    let mut guard = guard(&[]);
    let mut spy = Spy::default();
    let args = Arguments::parse(
        b"{}",
        &mut TokenStorage::<8>::new(),
        ArgumentLimits::default(),
    )
    .unwrap();
    let mut audit = NoAudit;
    let mut guarded = GuardedExecutor::new(
        &mut spy,
        "c1",
        Authorization::new(&mut guard, &mut audit, 1),
    );
    assert!(matches!(
        guarded.execute("read", args, &mut [0; 16]),
        Err(Error::Guard(_))
    ));
    assert_eq!(
        guarded.last_verdict().unwrap().reason,
        Reason::NoMatchingRule
    );
    assert_eq!(spy.calls, 0);
}

#[test]
fn plugin_router_is_gated_by_public_tool_name_before_backend_dispatch() {
    let mut backend = Spy::default();
    let routes = [Route::new("public.read", 0, "backend.operation")];
    let mut slots = [Slot::new(&mut backend)];
    let mut router = Router::new(&routes, &mut slots).unwrap();
    let call = prepared("c1", "public.read", b"{}");
    let mut deny = guard(&[]);
    assert!(matches!(
        call.execute_guarded(
            &mut router,
            &mut [0; 16],
            Authorization::new(&mut deny, &mut NoAudit, 1)
        ),
        Err(Error::Guard(_))
    ));
    let rules = [Rule::new(
        1,
        ActionKind::Tool,
        "public.read",
        Decision::Allow,
    )];
    let mut allow = guard(&rules);
    call.execute_guarded(
        &mut router,
        &mut [0; 16],
        Authorization::new(&mut allow, &mut NoAudit, 1),
    )
    .unwrap();
    assert_eq!(backend.calls, 1);
    assert_eq!(backend.name, "backend.operation");
}

#[derive(Default)]
struct CommandSpy {
    calls: usize,
    program: String,
    args: Vec<String>,
    cwd: Option<String>,
}
impl CommandExecutor for CommandSpy {
    fn execute(
        &mut self,
        command: Command<'_>,
        stdout: &mut [u8],
        stderr: &mut [u8],
    ) -> Result<CommandOutput, CommandError> {
        self.calls += 1;
        self.program = command.program.to_string();
        self.args = command.args.iter().map(|arg| arg.to_string()).collect();
        self.cwd = command.cwd.map(str::to_string);
        stdout[..6].copy_from_slice(b"stdout");
        stderr[..6].copy_from_slice(b"stderr");
        Ok(CommandOutput {
            status: ExitStatus::Exited(7),
            stdout_len: 6,
            stderr_len: 6,
        })
    }
}

#[test]
fn command_approval_binds_argv_boundaries_cwd_program_and_id() {
    let rules = [
        Rule::new(
            1,
            ActionKind::Command,
            "/bin/tool",
            Decision::RequireApproval,
        ),
        Rule::new(
            2,
            ActionKind::Command,
            "/bin/other",
            Decision::RequireApproval,
        ),
    ];
    let original_args = ["a", "bc"];
    let changed_args = ["ab", "c"];
    let mut guard = guard(&rules);
    let mut spy = CommandSpy::default();
    let mut stdout = [3; 16];
    let mut stderr = [4; 16];
    for command in [
        Command::new("c1", "/bin/tool", &changed_args).with_cwd("/work"),
        Command::new("c1", "/bin/tool", &original_args).with_cwd("/other"),
        Command::new("c1", "/bin/other", &original_args).with_cwd("/work"),
        Command::new("changed", "/bin/tool", &original_args).with_cwd("/work"),
    ] {
        let approval = guard
            .approve(
                Request::command("c1", "/bin/tool", &original_args, Some("/work")),
                10,
            )
            .unwrap();
        let error = command
            .execute_guarded(
                &mut spy,
                &mut stdout,
                &mut stderr,
                Authorization::new(&mut guard, &mut NoAudit, 1).with_approval(approval),
            )
            .unwrap_err();
        assert!(matches!(error, CommandError::Guard(v) if v.reason == Reason::ApprovalMismatch));
    }
    assert_eq!(spy.calls, 0);
    assert_eq!(stdout, [3; 16]);
    assert_eq!(stderr, [4; 16]);
}

#[test]
fn allowed_command_preserves_nonzero_exit_and_binary_output_boundaries() {
    let rules = [Rule::new(
        1,
        ActionKind::Command,
        "/bin/tool",
        Decision::Allow,
    )];
    let mut guard = guard(&rules);
    let mut spy = CommandSpy::default();
    let mut audit = AuditLog::default();
    let mut stdout = [0; 16];
    let mut stderr = [0; 16];
    let result = Command::new("cmd1", "/bin/tool", &["literal", "two words"])
        .with_cwd("/work")
        .execute_guarded(
            &mut spy,
            &mut stdout,
            &mut stderr,
            Authorization::new(&mut guard, &mut audit, 1),
        )
        .unwrap();
    assert_eq!(result.status, ExitStatus::Exited(7));
    assert_eq!(result.stdout, b"stdout");
    assert_eq!(result.stderr, b"stderr");
    assert_eq!(result.command.id, "cmd1");
    assert_eq!(spy.calls, 1);
    assert_eq!(spy.args, ["literal", "two words"]);
    assert_eq!(spy.cwd.as_deref(), Some("/work"));
    assert_eq!(guard.calls(), 1);
    assert_eq!(audit.events.len(), 1);
}

#[test]
fn tool_allow_rules_never_grant_command_permission() {
    let rules = [Rule::new(1, ActionKind::Tool, "runner", Decision::Allow)];
    let mut guard = guard(&rules);
    let mut spy = CommandSpy::default();
    let error = Command::new("c1", "runner", &[])
        .execute_guarded(
            &mut spy,
            &mut [0; 16],
            &mut [0; 16],
            Authorization::new(&mut guard, &mut NoAudit, 1),
        )
        .unwrap_err();
    assert!(matches!(error, CommandError::Guard(v) if v.reason == Reason::NoMatchingRule));
    assert_eq!(spy.calls, 0);
}

#[test]
fn command_audit_failure_blocks_both_output_streams() {
    let rules = [Rule::new(1, ActionKind::Command, "runner", Decision::Allow)];
    let mut guard = guard(&rules);
    let mut audit = AuditLog {
        fail: true,
        ..AuditLog::default()
    };
    let mut spy = CommandSpy::default();
    let mut stdout = [1; 16];
    let mut stderr = [2; 16];
    let error = Command::new("c1", "runner", &[])
        .execute_guarded(
            &mut spy,
            &mut stdout,
            &mut stderr,
            Authorization::new(&mut guard, &mut audit, 1),
        )
        .unwrap_err();
    assert!(matches!(error, CommandError::Guard(v) if v.reason == Reason::AuditFailed));
    assert_eq!(spy.calls, 0);
    assert_eq!(stdout, [1; 16]);
    assert_eq!(stderr, [2; 16]);
}

#[test]
fn guarded_executor_is_single_attempt_even_after_allow() {
    let rules = [Rule::new(1, ActionKind::Tool, "read", Decision::Allow)];
    let mut guard = guard(&rules);
    let mut audit = AuditLog::default();
    let mut spy = Spy::default();
    let args = Arguments::parse(
        b"{}",
        &mut TokenStorage::<8>::new(),
        ArgumentLimits::default(),
    )
    .unwrap();
    let mut wrapper = GuardedExecutor::new(
        &mut spy,
        "c1",
        Authorization::new(&mut guard, &mut audit, 1),
    );
    wrapper.execute("read", args, &mut [0; 16]).unwrap();
    assert!(matches!(
        wrapper.execute("read", args, &mut [0; 16]),
        Err(Error::InvalidState)
    ));
    assert_eq!(spy.calls, 1);
    assert_eq!(guard.calls(), 1);
    assert_eq!(audit.events.len(), 1);
}

#[test]
fn invalid_command_consumes_single_attempt_and_approval() {
    let rules = [Rule::new(
        1,
        ActionKind::Command,
        "runner",
        Decision::RequireApproval,
    )];
    let mut guard = guard(&rules);
    let mut audit = AuditLog::default();
    let mut spy = CommandSpy::default();
    let approval = guard
        .approve(Request::command("c1", "runner", &[], None), 10)
        .unwrap();
    let mut wrapper = GuardedCommandExecutor::new(
        &mut spy,
        Authorization::new(&mut guard, &mut audit, 1).with_approval(approval),
    );
    assert!(matches!(
        wrapper.execute(Command::new("", "runner", &[]), &mut [0; 16], &mut [0; 16]),
        Err(CommandError::InvalidCommand)
    ));
    assert!(matches!(
        wrapper.execute(
            Command::new("c1", "runner", &[]),
            &mut [0; 16],
            &mut [0; 16]
        ),
        Err(CommandError::InvalidState)
    ));
    assert_eq!(spy.calls, 0);
    assert_eq!(guard.calls(), 0);
    assert!(audit.events.is_empty());
}
