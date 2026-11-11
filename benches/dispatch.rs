//! Reproducible lookup and prepare/execute microbenchmarks; setup is excluded.
use std::{alloc::{GlobalAlloc, Layout, System}, hint::black_box, sync::atomic::{AtomicUsize, Ordering}, time::Instant};
use zharness::{Error, Executor, Invocation, Tool, ToolOutput, ToolSet, validation::{ArgumentLimits, Arguments}, zcall::v1::TokenStorage};

struct CountingAllocator;
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
// SAFETY: every operation delegates the identical pointer/layout/size to System.
// No ownership or alignment transformation occurs. This is benchmark-only code;
// the published library forbids unsafe Rust and contains no allocator.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) { unsafe { System.dealloc(ptr, layout) } }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn measure(name: &str, size: usize, memory: usize, mut operation: impl FnMut()) {
    let smoke = std::env::var_os("ZHARNESS_BENCH_SMOKE").is_some() || std::env::args().any(|a| a == "--test");
    let samples = if smoke { 31 } else { 1001 };
    let batch = if smoke { 32 } else { 4096 };
    let mut timings = Vec::with_capacity(samples);
    for _ in 0..128 { operation(); }
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let start = Instant::now();
    for _ in 0..batch { operation(); }
    let mean = start.elapsed().as_nanos() as f64 / batch as f64;
    for _ in 0..samples {
        let start = Instant::now(); operation(); timings.push(start.elapsed().as_nanos());
    }
    let allocations = ALLOCATIONS.load(Ordering::Relaxed) - before;
    assert_eq!(allocations, 0, "allocation in measured operation: {name}");
    timings.sort_unstable();
    let p = |percent: usize| timings[(timings.len() - 1) * percent / 100];
    println!("{name},{size},{mean:.2},{},{},{},{:.2},{allocations},{memory}", p(50), p(95), p(99), 1e9 / mean.max(1.0));
}
struct Echo;
impl Executor for Echo {
    fn execute(&mut self, _: &str, arguments: Arguments<'_>, output: &mut [u8]) -> Result<ToolOutput, Error> {
        let raw = arguments.as_bytes();
        output.get_mut(..raw.len()).ok_or(Error::Capacity { needed: raw.len() })?.copy_from_slice(raw);
        Ok(ToolOutput::success(raw.len()))
    }
}
fn main() {
    println!("case,size,mean_ns,p50_instrumented_ns,p95_instrumented_ns,p99_instrumented_ns,ops_per_second,total_allocations,workspace_bytes");
    for count in [1, 4, 16, 64, 256, 1024] {
        let names: Vec<String> = (0..count).map(|i| format!("tool-{i}")).collect();
        let definitions: Vec<Tool<'_>> = names.iter().map(|name| Tool::new(name, "benchmark tool", r#"{"type":"object"}"#)).collect();
        let set = ToolSet::new(&definitions).unwrap();
        let mut indexes = vec![0; count]; let indexed = set.indexed(&mut indexes).unwrap();
        let target = names.last().unwrap().as_str();
        let descriptors = std::mem::size_of_val(definitions.as_slice());
        measure("linear_lookup", count, descriptors + std::mem::size_of_val(&set), || { black_box(set.get(black_box(target))); });
        measure("indexed_lookup", count, descriptors + indexed.memory_bytes(), || { black_box(indexed.get(black_box(target))); });
    }
    let definitions = [Tool::new("echo", "Echo", r#"{"type":"object"}"#)]; let tools = ToolSet::new(&definitions).unwrap();
    for size in [64, 256, 1024, 4096, 65536] {
        let raw = format!("{{\"x\":\"{}\"}}", "x".repeat(size - 8));
        assert_eq!(raw.len(), size);
        let call = Invocation { call_id: "real-call-id", name: "echo", arguments: raw.as_bytes() };
        let mut tokens = TokenStorage::<32>::new(); let mut output = vec![0; size]; let mut echo = Echo;
        let memory = tokens.memory_bytes() + output.len();
        measure("prepare_execute", size, memory, || {
            let prepared = call.prepare(&tools, &mut tokens, ArgumentLimits::default()).unwrap();
            black_box(prepared.execute(&mut echo, &mut output).unwrap().text.len());
        });
    }
}
