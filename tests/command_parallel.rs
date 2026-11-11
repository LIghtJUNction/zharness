use zharness::{
    command::{Command, CommandError, CommandExecutor, CommandOutput, CommandResult, ExitStatus},
    hooks::{HookResult, Hooks},
};

#[test]
fn parallel_command_adapters_keep_hooks_ids_and_buffers_independent() {
    struct Worker;
    impl CommandExecutor for Worker {
        fn execute(
            &mut self,
            cmd: Command<'_>,
            out: &mut [u8],
            _: &mut [u8],
        ) -> Result<CommandOutput, CommandError> {
            out[..cmd.id.len()].copy_from_slice(cmd.id.as_bytes());
            Ok(CommandOutput {
                status: ExitStatus::Exited(0),
                stdout_len: cmd.id.len(),
                stderr_len: 0,
            })
        }
    }
    struct Audit {
        id: &'static str,
        count: usize,
    }
    impl Hooks for Audit {
        fn after_command(&mut self, result: CommandResult<'_, '_>) {
            assert_eq!(result.command.id, self.id);
            self.count += 1;
        }
    }
    let mut a = [0; 32];
    let mut b = [0; 32];
    let run = |id: &'static str, buffer: &mut [u8]| {
        let mut hooks = Audit { id, count: 0 };
        let HookResult::Applied(result) = Command::new(id, "adapter", &[])
            .execute_with_hooks(&mut Worker, buffer, &mut [], &mut hooks)
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(result.stdout, id.as_bytes());
        assert_eq!(hooks.count, 1);
        result.stdout.len()
    };
    let (na, nb) = std::thread::scope(|s| {
        let first = s.spawn(|| run("command-a", &mut a));
        let second = s.spawn(|| run("command-b", &mut b));
        (first.join().unwrap(), second.join().unwrap())
    });
    assert_eq!(&a[..na], b"command-a");
    assert_eq!(&b[..nb], b"command-b");
}
