use std::cell::RefCell;
use zharness::{Error, Executor, Invocation, Tool, ToolOutput, ToolSet, command::{Command, CommandError, CommandExecutor, CommandFailure, CommandOutput, CommandResult, ExitStatus, OutputStream}, execution::{AfterExecute, BeforeExecute, ExecuteError}, hooks::{AfterResult, BeforeResult, Decision, HookResult, Hooks, NoHooks}, output::Trim, validation::{ArgumentLimits, Arguments}, zcall::v1::TokenStorage};

struct Audit<'a> { log: &'a RefCell<Vec<String>>, label: &'static str, decision: Decision }
impl Audit<'_> {
    fn record(&self, kind: &str, id: &str) { self.log.borrow_mut().push(format!("{}:{kind}:{id}", self.label)); }
}
impl Hooks for Audit<'_> {
    fn before_execute(&mut self, event: BeforeExecute<'_>) -> Decision { assert_eq!(event.arguments.as_bytes(), b"{}"); self.record("before", event.call_id); self.decision }
    fn after_execute(&mut self, event: AfterExecute<'_>) { assert_eq!(event.call.call_id, event.completion.call_id); self.record(if event.completion.is_error { "tool-error" } else { "after" }, event.call.call_id); }
    fn execute_error(&mut self, event: ExecuteError<'_>) { self.record("failed", event.call.call_id); }
    fn before_command(&mut self, command: Command<'_>) -> Decision { assert_eq!(command.args, &["literal ; not a shell"]); self.record("before", command.id); self.decision }
    fn after_command(&mut self, result: CommandResult<'_, '_>) { self.record(if result.status.success() { "after" } else { "exit-error" }, result.command.id); }
    fn command_error(&mut self, event: CommandFailure<'_>) { self.record("failed", event.command.id); }
    fn before_result(&mut self, e: BeforeResult<'_>) -> Decision { self.record("reduce", e.meta.call_id); Decision::Allow }
    fn after_result(&mut self, e: AfterResult<'_>) { self.record("reduced", e.meta.call_id); }
}
#[derive(Clone, Copy)]
enum Mode { Success, BusinessFailure, Error(Error), Oversized, InvalidUtf8 }
struct ToolExec<'a> { log: &'a RefCell<Vec<String>>, mode: Mode, calls: usize }
impl Executor for ToolExec<'_> {
    fn execute(&mut self, _: &str, _: Arguments<'_>, out: &mut [u8]) -> Result<ToolOutput, Error> {
        self.calls += 1; self.log.borrow_mut().push("execute".into());
        match self.mode {
            Mode::Error(e) => Err(e),
            Mode::Oversized => Ok(ToolOutput::success(usize::MAX)),
            Mode::InvalidUtf8 => { out[0] = 255; Ok(ToolOutput::success(1)) }
            _ => { out[..4].copy_from_slice(b" ok "); Ok(ToolOutput { written: 4, is_error: matches!(self.mode, Mode::BusinessFailure) }) }
        }
    }
}
static TOOLS: &[Tool<'static>] = &[Tool::new("test", "test", r#"{"type":"object"}"#)];
fn prepared() -> zharness::bridge::Prepared<'static> {
    Invocation { call_id: "real-1", name: "test", arguments: b"{}" }.prepare(&ToolSet::new(TOOLS).unwrap(), &mut TokenStorage::<16>::new(), ArgumentLimits::default()).unwrap()
}
#[test]
fn actual_executor_is_bracketed_before_separate_output_processing() {
    let log = RefCell::new(Vec::new()); let mut exec = ToolExec { log: &log, mode: Mode::Success, calls: 0 };
    let mut hooks = (Audit { log: &log, label: "a", decision: Decision::Allow }, Audit { log: &log, label: "b", decision: Decision::Allow });
    let mut output = [0; 64]; let mut reduced = [0; 64];
    let HookResult::Applied(result) = prepared().execute_with_hooks(&mut exec, &mut output, &mut &mut hooks).unwrap() else { panic!() };
    assert_eq!(result.call_id, "real-1");
    let HookResult::Applied(result) = result.transform_with_hooks("test", &mut Trim, &mut reduced, &mut hooks).unwrap() else { panic!() };
    assert_eq!(result.text, "ok"); assert_eq!(exec.calls, 1);
    assert_eq!(*log.borrow(), ["a:before:real-1", "b:before:real-1", "execute", "a:after:real-1", "b:after:real-1", "a:reduce:real-1", "b:reduce:real-1", "a:reduced:real-1", "b:reduced:real-1"]);
}
#[test]
fn paused_and_denied_execution_never_touches_executor_or_output() {
    for decision in [Decision::Pause, Decision::Deny] {
        let log = RefCell::new(Vec::new()); let mut exec = ToolExec { log: &log, mode: Mode::Success, calls: 0 };
        let mut hooks = (Audit { log: &log, label: "a", decision }, Audit { log: &log, label: "b", decision: Decision::Allow });
        let mut output = [0xaa; 64];
        let result = prepared().execute_with_hooks(&mut exec, &mut output, &mut hooks);
        match decision { Decision::Pause => assert!(matches!(result, Ok(HookResult::Paused))), Decision::Deny => assert!(matches!(result, Err(Error::Denied))), _ => unreachable!() }
        assert_eq!(exec.calls, 0); assert_eq!(output, [0xaa; 64]); assert_eq!(*log.borrow(), ["a:before:real-1"]);
        hooks.0.decision = Decision::Allow;
        assert!(matches!(prepared().execute_with_hooks(&mut exec, &mut output, &mut hooks), Ok(HookResult::Applied(_)))); assert_eq!(exec.calls, 1);
    }
}
#[test]
fn tool_business_failures_preserve_status_and_execution_errors_are_observed_once() {
    for mode in [Mode::BusinessFailure, Mode::Error(Error::ExecutionFailed), Mode::Error(Error::Capacity { needed: 100 }), Mode::Oversized, Mode::InvalidUtf8] {
        let log = RefCell::new(Vec::new()); let mut exec = ToolExec { log: &log, mode, calls: 0 };
        let mut hooks = (Audit { log: &log, label: "a", decision: Decision::Allow }, Audit { log: &log, label: "b", decision: Decision::Allow });
        let mut output = [0; 64]; let result = prepared().execute_with_hooks(&mut exec, &mut output, &mut hooks);
        if matches!(mode, Mode::BusinessFailure) {
            let HookResult::Applied(c) = result.unwrap() else { panic!() }; assert!(c.is_error); assert_eq!(log.borrow()[3], "a:tool-error:real-1");
        } else {
            assert!(result.is_err()); assert_eq!(log.borrow()[3], "a:failed:real-1"); assert_eq!(log.borrow()[4], "b:failed:real-1");
        }
        assert_eq!(exec.calls, 1); assert_eq!(log.borrow().len(), 5);
    }
}
struct FakeCommand<'a> { log: &'a RefCell<Vec<String>>, calls: usize, response: Result<CommandOutput, CommandError> }
impl CommandExecutor for FakeCommand<'_> {
    fn execute(&mut self, command: Command<'_>, stdout: &mut [u8], stderr: &mut [u8]) -> Result<CommandOutput, CommandError> {
        self.calls += 1; self.log.borrow_mut().push("execute".into());
        assert_eq!(command.program, "approved"); assert_eq!(command.cwd, Some("workspace"));
        stdout[..2].copy_from_slice(&[0xff, 0]); stderr[..3].copy_from_slice(b"err"); self.response
    }
}
fn command() -> Command<'static> { Command::new("cmd-7", "approved", &["literal ; not a shell"]).with_cwd("workspace") }
#[test]
fn command_hooks_receive_exact_metadata_binary_output_and_exit_status() {
    for status in [ExitStatus::Exited(0), ExitStatus::Exited(7), ExitStatus::Terminated] {
        let log = RefCell::new(Vec::new()); let mut executor = FakeCommand { log: &log, calls: 0, response: Ok(CommandOutput { status, stdout_len: 2, stderr_len: 3 }) };
        let mut hooks = (Audit { log: &log, label: "a", decision: Decision::Allow }, Audit { log: &log, label: "b", decision: Decision::Allow });
        let mut out = [0; 16]; let mut err = [0; 16];
        let HookResult::Applied(result) = command().execute_with_hooks(&mut executor, &mut out, &mut err, &mut &mut hooks).unwrap() else { panic!() };
        assert_eq!(result.command.id, "cmd-7"); assert_eq!(result.status, status); assert_eq!(result.stdout, &[0xff, 0]); assert_eq!(result.stderr, b"err");
        assert_eq!(executor.calls, 1); assert_eq!(log.borrow().len(), 5); assert_eq!(log.borrow()[2], "execute");
        assert_eq!(log.borrow()[3], if status.success() { "a:after:cmd-7" } else { "a:exit-error:cmd-7" });
    }
}
#[test]
fn command_denial_and_pause_are_not_failed_process_attempts() {
    for decision in [Decision::Pause, Decision::Deny] {
        let log = RefCell::new(Vec::new()); let mut executor = FakeCommand { log: &log, calls: 0, response: Err(CommandError::Spawn) };
        let mut hooks = (Audit { log: &log, label: "a", decision }, Audit { log: &log, label: "b", decision: Decision::Allow });
        let mut out = [0xaa; 16]; let mut err = [0xbb; 16];
        let result = command().execute_with_hooks(&mut executor, &mut out, &mut err, &mut hooks);
        if decision == Decision::Pause { assert!(matches!(result, Ok(HookResult::Paused))); } else { assert!(matches!(result, Err(CommandError::Denied))); }
        assert_eq!(executor.calls, 0); assert_eq!(out, [0xaa; 16]); assert_eq!(err, [0xbb; 16]); assert_eq!(*log.borrow(), ["a:before:cmd-7"]);
    }
}
#[test]
fn adapter_failures_and_invalid_capture_lengths_are_reported_without_retry() {
    let responses = [Err(CommandError::Spawn), Err(CommandError::Io), Err(CommandError::TimedOut), Err(CommandError::Cancelled), Err(CommandError::Capacity { stream: OutputStream::Stderr, needed: 100 }), Ok(CommandOutput { status: ExitStatus::Exited(0), stdout_len: usize::MAX, stderr_len: 0 }), Ok(CommandOutput { status: ExitStatus::Exited(0), stdout_len: 0, stderr_len: usize::MAX })];
    for response in responses {
        let log = RefCell::new(Vec::new()); let mut executor = FakeCommand { log: &log, calls: 0, response };
        let mut hooks = (Audit { log: &log, label: "a", decision: Decision::Allow }, Audit { log: &log, label: "b", decision: Decision::Allow });
        let error = command().execute_with_hooks(&mut executor, &mut [0; 16], &mut [0; 16], &mut hooks).unwrap_err();
        assert_eq!(error, response.err().unwrap_or(CommandError::InvalidOutput)); assert_eq!(executor.calls, 1);
        assert_eq!(*log.borrow(), ["a:before:cmd-7", "b:before:cmd-7", "execute", "a:failed:cmd-7", "b:failed:cmd-7"]);
    }
}
#[test]
fn invalid_command_never_reaches_hooks_or_adapter() {
    let bad = [Command::new("", "program", &[]), Command::new("id", "", &[]), Command::new("id", "x\0y", &[]), Command::new("id", "p", &["x\0"]), Command::new("id", "p", &[]).with_cwd("")];
    let log = RefCell::new(Vec::new()); let mut exec = FakeCommand { log: &log, calls: 0, response: Err(CommandError::Spawn) };
    let mut hooks = Audit { log: &log, label: "a", decision: Decision::Allow };
    for cmd in bad { assert!(matches!(cmd.execute_with_hooks(&mut exec, &mut [], &mut [], &mut hooks), Err(CommandError::InvalidCommand))); }
    assert_eq!(exec.calls, 0); assert!(log.borrow().is_empty());
    assert!(Command::new("id", "a program", &["", "中文", "$x;* "]).validate().is_ok());
}
#[test]
fn legacy_hooks_still_compile_with_default_execution_callbacks() {
    struct OldHooks;
    impl Hooks for OldHooks { fn before(&mut self, _: zharness::hooks::Before<'_>) -> Decision { Decision::Allow } }
    let log = RefCell::new(Vec::new()); let mut exec = ToolExec { log: &log, mode: Mode::Success, calls: 0 };
    assert!(matches!(prepared().execute_with_hooks(&mut exec, &mut [0; 64], &mut OldHooks), Ok(HookResult::Applied(_))));
    assert!(matches!(prepared().execute_with_hooks(&mut exec, &mut [0; 64], &mut NoHooks), Ok(HookResult::Applied(_))));
}
