//! Steady calls through a real dynamic library. Building, loading and binding
//! happen before measurement; allocation counters cover this host executable.
#[path = "../tests/support/plugin_build.rs"]
mod plugin_build;

use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};
use zharness::{
    Invocation, Tool, ToolSet,
    execution::{AfterExecute, BeforeExecute},
    hooks::{Decision, HookResult, Hooks},
    plugin::{ExtensionApi, Plugin, UnloadPolicy},
    validation::ArgumentLimits,
    zcall::v1::TokenStorage,
};

struct Counter;
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
// SAFETY: pointers, layouts and sizes are forwarded unchanged to System.
// This benchmark-only allocator does not intercept the cdylib's allocator.
unsafe impl GlobalAlloc for Counter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(pointer, layout, size) }
    }
}
#[global_allocator]
static COUNTER: Counter = Counter;

fn measure(name: &str, bytes: usize, proxy_bytes: usize, mut operation: impl FnMut()) {
    let smoke = std::env::var_os("ZHARNESS_BENCH_SMOKE").is_some()
        || std::env::args().any(|argument| argument == "--test");
    let samples = if smoke { 31 } else { 1001 };
    let batch = if smoke { 32 } else { 4096 };
    let mut timings = Vec::with_capacity(samples);
    for _ in 0..32 {
        operation();
    }
    let before = ALLOCS.load(Ordering::Relaxed);
    let start = Instant::now();
    for _ in 0..batch {
        operation();
    }
    let mean = start.elapsed().as_nanos() as f64 / batch as f64;
    for _ in 0..samples {
        let start = Instant::now();
        operation();
        timings.push(start.elapsed().as_nanos());
    }
    let allocations = ALLOCS.load(Ordering::Relaxed) - before;
    assert_eq!(
        allocations, 0,
        "{name} allocated in the measured host region"
    );
    timings.sort_unstable();
    let percentile = |p: usize| timings[(timings.len() - 1) * p / 100];
    println!(
        "{name},{bytes},{proxy_bytes},{mean:.2},{},{},{},{allocations}",
        percentile(50),
        percentile(95),
        percentile(99)
    );
}

#[derive(Default)]
struct Audit(u64);
impl Hooks for Audit {
    fn before_execute(&mut self, event: BeforeExecute<'_>) -> Decision {
        black_box(event.call_id);
        self.0 += 1;
        Decision::Allow
    }
    fn after_execute(&mut self, event: AfterExecute<'_>) {
        black_box(event.completion.text);
        self.0 += 1;
    }
}

fn main() {
    let path = plugin_build::build_example("native_plugin");
    // SAFETY: this trusted fixture follows zplugin's ABI and cooperative-unload
    // contract; it retains no tasks, callbacks or other activity outside its proxy.
    let library = unsafe { Plugin::load_path(path, UnloadPolicy::Cooperative) }.unwrap();
    let mut extension = ExtensionApi::create(&library).unwrap();
    let proxy_bytes = std::mem::size_of_val(&extension);
    let tools = [Tool::new("echo", "Echo", r#"{"type":"object"}"#)];
    let registry = ToolSet::new(&tools).unwrap();
    println!(
        "case,bytes,proxy_state_bytes,mean_ns,p50_instrumented_ns,p95_instrumented_ns,p99_instrumented_ns,host_allocations"
    );
    for size in [64, 1024, 4096] {
        let input = format!("{{\"x\":\"{}\"}}", "x".repeat(size - 8));
        assert_eq!(input.len(), size);
        let mut output = vec![0; size];
        assert_eq!(
            extension
                .call_bytes("echo", input.as_bytes(), &mut output)
                .unwrap(),
            input.as_bytes()
        );
        let invocation = Invocation {
            call_id: "plugin-benchmark",
            name: "echo",
            arguments: input.as_bytes(),
        };
        let prepared = invocation
            .prepare(
                &registry,
                &mut TokenStorage::<16>::new(),
                ArgumentLimits::default(),
            )
            .unwrap();

        measure("generated_call", size, proxy_bytes, || {
            let written = extension
                .call(
                    black_box("echo"),
                    black_box(input.as_bytes()),
                    black_box(&mut output),
                )
                .unwrap();
            black_box(written);
            black_box(output.as_slice());
        });
        measure("checked_bytes", size, proxy_bytes, || {
            black_box(
                extension
                    .call_bytes(
                        black_box("echo"),
                        black_box(input.as_bytes()),
                        black_box(&mut output),
                    )
                    .unwrap(),
            );
        });
        measure("prepared_executor", size, proxy_bytes, || {
            let completion = black_box(prepared)
                .execute(black_box(&mut extension), black_box(&mut output))
                .unwrap();
            black_box(completion.text);
        });
        let mut hooks = (Audit::default(), Audit::default());
        measure("executor_two_hooks", size, proxy_bytes, || {
            let HookResult::Applied(completion) = black_box(prepared)
                .execute_with_hooks(
                    black_box(&mut extension),
                    black_box(&mut output),
                    black_box(&mut hooks),
                )
                .unwrap()
            else {
                panic!("benchmark hooks always allow");
            };
            black_box(completion.text);
            black_box(&hooks);
        });
    }
    drop(extension);
    assert!(
        library.try_close().is_ok(),
        "benchmark fixture must quiesce"
    );
}
