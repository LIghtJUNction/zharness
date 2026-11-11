//! Borrowed command descriptions and hooked execution boundaries. No process is
//! spawned by this module. An application supplies a trusted CommandExecutor.
//!
//! Arguments are literal elements, not a shell command string. The descriptor
//! is UTF-8 and intentionally platform-neutral. OS environment, permissions,
//! sandboxing, I/O scheduling and timeout enforcement belong to the adapter.
use crate::hooks::{Decision, HookResult, Hooks};

/// An application-assigned command identity, executable, literal arguments and
/// optional working directory. IDs are correlation data, not authentication.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Command<'a> {
    pub id: &'a str,
    pub program: &'a str,
    pub args: &'a [&'a str],
    pub cwd: Option<&'a str>,
}
impl<'a> Command<'a> {
    pub const fn new(id: &'a str, program: &'a str, args: &'a [&'a str]) -> Self {
        Self { id, program, args, cwd: None }
    }
    pub const fn with_cwd(mut self, cwd: &'a str) -> Self { self.cwd = Some(cwd); self }
    /// Structural checks only, not executable resolution or an allowlist policy.
    pub fn validate(self) -> Result<(), CommandError> {
        if self.id.is_empty() || self.program.is_empty() || self.id.contains('\0') || self.program.contains('\0')
            || self.args.iter().any(|arg| arg.contains('\0'))
            || self.cwd.is_some_and(|cwd| cwd.is_empty() || cwd.contains('\0')) {
            return Err(CommandError::InvalidCommand);
        }
        Ok(())
    }
    /// Validate and gate immediately before invoking the adapter once. Returning
    /// an error never retries the process. Preserve/inspect external effects even
    /// after timeout, cancellation, I/O errors or insufficient capture capacity.
    pub fn execute_with_hooks<'o, E: CommandExecutor>(
        self,
        executor: &mut E,
        stdout: &'o mut [u8],
        stderr: &'o mut [u8],
        hooks: &mut impl Hooks,
    ) -> Result<HookResult<CommandResult<'a, 'o>>, CommandError> {
        self.validate()?;
        match hooks.before_command(self) {
            Decision::Allow => {}
            Decision::Pause => return Ok(HookResult::Paused),
            Decision::Deny => return Err(CommandError::Denied),
        }
        let output = match executor.execute(self, stdout, stderr) {
            Ok(output) => output,
            Err(error) => {
                hooks.command_error(CommandFailure { command: self, error });
                return Err(error);
            }
        };
        // Process output can be arbitrary bytes. Do not lose binary data by
        // coercing it to UTF-8 or confusing nonzero exit with capture failure.
        let (Some(stdout), Some(stderr)) = (stdout.get(..output.stdout_len), stderr.get(..output.stderr_len)) else {
            let error = CommandError::InvalidOutput;
            hooks.command_error(CommandFailure { command: self, error });
            return Err(error);
        };
        let result = CommandResult { command: self, status: output.status, stdout, stderr };
        hooks.after_command(result);
        Ok(HookResult::Applied(result))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExitStatus {
    Exited(i32),
    /// Termination without a portable numeric exit code (for example a signal).
    Terminated,
}
impl ExitStatus {
    pub const fn success(self) -> bool { matches!(self, Self::Exited(0)) }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandOutput {
    pub status: ExitStatus,
    pub stdout_len: usize,
    pub stderr_len: usize,
}
/// A completed process result. ExitStatus::Exited(nonzero) is observable here,
/// not disguised as a successful exit or an automatic execution retry.
#[derive(Clone, Copy, Debug)]
pub struct CommandResult<'c, 'o> {
    pub command: Command<'c>,
    pub status: ExitStatus,
    pub stdout: &'o [u8],
    pub stderr: &'o [u8],
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputStream { Stdout, Stderr }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum CommandError {
    InvalidCommand,
    Denied,
    Spawn,
    Io,
    TimedOut,
    Cancelled,
    Capacity { stream: OutputStream, needed: usize },
    InvalidOutput,
    /// Adapter-defined bounded error code; rich OS diagnostics remain caller-owned.
    Adapter(u32),
}
impl core::fmt::Display for CommandError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { write!(f, "{self:?}") }
}
impl core::error::Error for CommandError {}
#[derive(Clone, Copy, Debug)]
pub struct CommandFailure<'a> {
    pub command: Command<'a>,
    pub error: CommandError,
}

/// The trusted adapter must honor the exact approved executable/argv/cwd, avoid
/// an implicit shell, bound its output, and report rather than hide failures.
/// All OS resources, timeout/cancellation policy and environment are caller-owned.
pub trait CommandExecutor {
    fn execute(&mut self, command: Command<'_>, stdout: &mut [u8], stderr: &mut [u8]) -> Result<CommandOutput, CommandError>;
}
