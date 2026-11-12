//! Measures local hook/adapter overhead, not process startup or network latency.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};
use zharness::{
    Error, Executor, Invocation, Tool, ToolOutput, ToolSet,
    command::{
        Command, CommandError, CommandExecutor, CommandOutput, CommandResult, ExitStatus,
        OutputStream,
    },
    execution::{AfterExecute, BeforeExecute},
    hooks::{Decision, HookResult, Hooks, NoHooks},
    validation::{ArgumentLimits, Arguments},
    zcall::v1::TokenStorage,
};
struct Counter;
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
// SAFETY: identical pointers/layouts/sizes are delegated to System. Instrumentation
// is confined to this benchmark binary; the library itself forbids unsafe code.
unsafe impl GlobalAlloc for Counter {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc_zeroed(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(p, l, n) }
    }
}
#[global_allocator]
static COUNTER: Counter = Counter;
fn measure(name: &str, size: usize, mut op: impl FnMut()) {
    let smoke = std::env::var_os("ZHARNESS_BENCH_SMOKE").is_some()
        || std::env::args().any(|a| a == "--test");
    let samples = if smoke { 31 } else { 1001 };
    let batch = if smoke { 32 } else { 4096 };
    let mut timings = Vec::with_capacity(samples);
    for _ in 0..32 {
        op();
    }
    let before = ALLOCS.load(Ordering::Relaxed);
    let start = Instant::now();
    for _ in 0..batch {
        op();
    }
    let mean = start.elapsed().as_nanos() as f64 / batch as f64;
    for _ in 0..samples {
        let start = Instant::now();
        op();
        timings.push(start.elapsed().as_nanos());
    }
    let allocations = ALLOCS.load(Ordering::Relaxed) - before;
    assert_eq!(allocations, 0);
    timings.sort_unstable();
    let p = |percent: usize| timings[(timings.len() - 1) * percent / 100];
    println!(
        "{name},{size},{mean:.2},{},{},{},{allocations}",
        p(50),
        p(95),
        p(99)
    );
}
struct ToolAdapter;
impl Executor for ToolAdapter {
    fn execute(
        &mut self,
        _: &str,
        args: Arguments<'_>,
        out: &mut [u8],
    ) -> Result<ToolOutput, Error> {
        let text = args.as_bytes();
        out.get_mut(..text.len())
            .ok_or(Error::Capacity { needed: text.len() })?
            .copy_from_slice(text);
        Ok(ToolOutput::success(text.len()))
    }
}
struct CommandAdapter<'a>(&'a [u8]);
impl CommandExecutor for CommandAdapter<'_> {
    fn execute(
        &mut self,
        _: Command<'_>,
        out: &mut [u8],
        _: &mut [u8],
    ) -> Result<CommandOutput, CommandError> {
        out.get_mut(..self.0.len())
            .ok_or(CommandError::Capacity {
                stream: OutputStream::Stdout,
                needed: self.0.len(),
            })?
            .copy_from_slice(self.0);
        Ok(CommandOutput {
            status: ExitStatus::Exited(0),
            stdout_len: self.0.len(),
            stderr_len: 0,
        })
    }
}
#[derive(Default)]
struct Audit(u64);
impl Hooks for Audit {
    fn before_execute(&mut self, e: BeforeExecute<'_>) -> Decision {
        black_box(e.call_id);
        self.0 += 1;
        Decision::Allow
    }
    fn after_execute(&mut self, e: AfterExecute<'_>) {
        black_box(e.completion.text);
        self.0 += 1;
    }
    fn before_command(&mut self, c: Command<'_>) -> Decision {
        black_box(c.program);
        self.0 += 1;
        Decision::Allow
    }
    fn after_command(&mut self, c: CommandResult<'_, '_>) {
        black_box(c.stdout);
        self.0 += 1;
    }
}
fn main() {
    println!(
        "case,bytes,mean_ns,p50_instrumented_ns,p95_instrumented_ns,p99_instrumented_ns,total_allocations"
    );
    let tools = [Tool::new("echo", "Echo", r#"{"type":"object"}"#)];
    let registry = ToolSet::new(&tools).unwrap();
    for size in [64, 1024, 4096] {
        let json = format!("{{\"x\":\"{}\"}}", "x".repeat(size - 8));
        let call = Invocation {
            signature: None,
            call_id: "call-1",
            name: "echo",
            arguments: json.as_bytes(),
        };
        let prepared = call
            .prepare(
                &registry,
                &mut TokenStorage::<16>::new(),
                ArgumentLimits::default(),
            )
            .unwrap();
        let mut buffer = vec![0; size];
        let mut executor = ToolAdapter;
        measure("tool_unhooked", size, || {
            let c = black_box(prepared)
                .execute(&mut executor, &mut buffer)
                .unwrap();
            black_box(c.text.as_bytes());
        });
        let mut hooks = (Audit::default(), Audit::default());
        measure("tool_execution_hooks", size, || {
            let HookResult::Applied(c) = black_box(prepared)
                .execute_with_hooks(&mut executor, &mut buffer, black_box(&mut hooks))
                .unwrap()
            else {
                panic!()
            };
            black_box(c.text.as_bytes());
            black_box(&hooks);
        });
        let command = Command::new("command-1", "application-adapter", &["literal"]);
        let mut executor = CommandAdapter(json.as_bytes());
        measure("command_no_hooks", size, || {
            let HookResult::Applied(c) = black_box(command)
                .execute_with_hooks(&mut executor, &mut buffer, &mut [], &mut NoHooks)
                .unwrap()
            else {
                panic!()
            };
            black_box(c.stdout);
        });
        measure("command_two_hooks", size, || {
            let HookResult::Applied(c) = black_box(command)
                .execute_with_hooks(&mut executor, &mut buffer, &mut [], black_box(&mut hooks))
                .unwrap()
            else {
                panic!()
            };
            black_box(c.stdout);
            black_box(&hooks);
        });
    }
}
