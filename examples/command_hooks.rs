//! Real OS execution stays in this std application, not in zharness.
//! Runs only this executable's controlled child mode; never invokes a shell.
use std::{io::{Read, Write}, process::{Command as Process, Stdio}};
use zharness::{command::{Command, CommandError, CommandExecutor, CommandFailure, CommandOutput, CommandResult, ExitStatus, OutputStream}, hooks::{Decision, HookResult, Hooks}};

/// Drain both pipes to EOF, even after the caller's capture buffer fills. This
/// avoids deadlock from a child blocked on a full pipe and bounds retained bytes.
/// Timeout/kill policy is not implemented by this demonstration adapter.
fn capture(mut pipe: impl Read, output: &mut [u8], stream: OutputStream) -> Result<usize, CommandError> {
    let mut chunk = [0; 4096]; let mut total = 0usize; let mut stored = 0;
    loop {
        let n = match pipe.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(CommandError::Io),
        };
        total = total.checked_add(n).ok_or(CommandError::Io)?;
        let keep = n.min(output.len() - stored);
        output[stored..stored + keep].copy_from_slice(&chunk[..keep]); stored += keep;
    }
    if total > output.len() { Err(CommandError::Capacity { stream, needed: total }) } else { Ok(stored) }
}
struct LocalProcess { attempts: usize }
impl CommandExecutor for LocalProcess {
    fn execute(&mut self, command: Command<'_>, stdout: &mut [u8], stderr: &mut [u8]) -> Result<CommandOutput, CommandError> {
        let mut process = Process::new(command.program);
        process.args(command.args).env_clear().stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        if let Some(cwd) = command.cwd { process.current_dir(cwd); }
        self.attempts += 1;
        let mut child = process.spawn().map_err(|_| CommandError::Spawn)?;
        let (Some(out_pipe), Some(err_pipe)) = (child.stdout.take(), child.stderr.take()) else {
            let _ = child.kill(); let _ = child.wait(); return Err(CommandError::Io);
        };
        let captured = std::thread::scope(|scope| {
            let reader = scope.spawn(|| capture(out_pipe, stdout, OutputStream::Stdout));
            let err = capture(err_pipe, stderr, OutputStream::Stderr);
            let out = reader.join().map_err(|_| CommandError::Io);
            (out, err)
        });
        // Always reap the child before propagating a capture-capacity error.
        let status = child.wait().map_err(|_| CommandError::Io)?;
        let stdout_len = captured.0??; let stderr_len = captured.1?;
        Ok(CommandOutput { status: status.code().map_or(ExitStatus::Terminated, ExitStatus::Exited), stdout_len, stderr_len })
    }
}
struct Approval<'a> { program: &'a str, approved: bool }
impl Hooks for Approval<'_> {
    fn before_command(&mut self, command: Command<'_>) -> Decision {
        if command.program != self.program || command.args.first() != Some(&"--child") { Decision::Deny }
        else if !self.approved { Decision::Pause }
        else { Decision::Allow }
    }
}
#[derive(Default)]
struct Audit { completed: usize, failed: usize, nonzero: usize }
impl Hooks for Audit {
    fn after_command(&mut self, result: CommandResult<'_, '_>) {
        self.completed += 1;
        if !result.status.success() { self.nonzero += 1; }
    }
    fn command_error(&mut self, _: CommandFailure<'_>) { self.failed += 1; }
}
fn child(mode: &str) -> Result<(), Box<dyn std::error::Error>> {
    let stdout = std::io::stdout(); let stderr = std::io::stderr();
    let mut out = stdout.lock(); let mut err = stderr.lock();
    if mode == "large" {
        for _ in 0..2048 { out.write_all(&[b'x'; 256])?; err.write_all(&[b'y'; 256])?; }
    } else { out.write_all(b"child output\n")?; err.write_all(b"child diagnostic\n")?; }
    out.flush()?; err.flush()?;
    if mode == "fail" { std::process::exit(7); }
    Ok(())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("--child") { return child(args.next().as_deref().unwrap_or("ok")); }
    let executable = std::env::current_exe()?;
    let program = executable.to_str().ok_or("this example requires a UTF-8 executable path")?;
    let mut process = LocalProcess { attempts: 0 };
    let mut hooks = (Approval { program, approved: false }, Audit::default());
    let mut stdout = [0xaa; 256]; let mut stderr = [0xbb; 256];
    let command = Command::new("command-1", program, &["--child", "ok"]);
    assert!(matches!(command.execute_with_hooks(&mut process, &mut stdout, &mut stderr, &mut hooks)?, HookResult::Paused));
    assert_eq!(process.attempts, 0); assert_eq!(stdout, [0xaa; 256]);
    hooks.0.approved = true;
    let HookResult::Applied(result) = command.execute_with_hooks(&mut process, &mut stdout, &mut stderr, &mut hooks)? else { return Err("unexpected pause".into()); };
    assert!(result.status.success()); assert_eq!(result.stdout, b"child output\n"); assert_eq!(result.stderr, b"child diagnostic\n");
    let HookResult::Applied(result) = Command::new("command-2", program, &["--child", "fail"]).execute_with_hooks(&mut process, &mut stdout, &mut stderr, &mut hooks)? else { return Err("unexpected pause".into()); };
    assert_eq!(result.status, ExitStatus::Exited(7));
    let error = Command::new("command-3", program, &["--child", "large"]).execute_with_hooks(&mut process, &mut [0; 16], &mut [0; 16], &mut hooks).unwrap_err();
    assert_eq!(error, CommandError::Capacity { stream: OutputStream::Stdout, needed: 524288 });
    assert_eq!(process.attempts, 3); assert_eq!(hooks.1.completed, 2); assert_eq!(hooks.1.nonzero, 1); assert_eq!(hooks.1.failed, 1);
    println!("Real child processes: pause prevented spawn; success and exit 7 observed; oversized pipes drained and failure reported without retry.");
    Ok(())
}
