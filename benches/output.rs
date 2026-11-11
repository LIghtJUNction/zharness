//! Full processing/validation measurements with allocation accounting.
use std::{alloc::{GlobalAlloc, Layout, System}, hint::black_box, sync::atomic::{AtomicUsize, Ordering}, time::Instant};
use zharness::{bridge::Completion, hooks::{HookResult, NoHooks}, output::{Chain, DedupAdjacentLines, Identity, JsonMinify, Trim, Truncate}, zcall::v1::{Limits, TokenStorage}};
struct Counter;
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
// SAFETY: pointer/layout/size arguments are forwarded unchanged to System.
// This test-only instrumentation is not part of the no_std library.
unsafe impl GlobalAlloc for Counter {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 { ALLOCS.fetch_add(1, Ordering::Relaxed); unsafe { System.alloc(l) } }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 { ALLOCS.fetch_add(1, Ordering::Relaxed); unsafe { System.alloc_zeroed(l) } }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) { unsafe { System.dealloc(p, l) } }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 { ALLOCS.fetch_add(1, Ordering::Relaxed); unsafe { System.realloc(p, l, n) } }
}
#[global_allocator]
static ALLOCATOR: Counter = Counter;
fn measure(name: &str, size: usize, memory: usize, mut operation: impl FnMut()) {
    let smoke = std::env::var_os("ZHARNESS_BENCH_SMOKE").is_some() || std::env::args().any(|a| a == "--test");
    let samples = if smoke { 31 } else { 1001 }; let batch = if smoke { 32 } else { 4096 };
    let mut times = Vec::with_capacity(samples);
    for _ in 0..32 { operation(); }
    let before = ALLOCS.load(Ordering::Relaxed); let start = Instant::now();
    for _ in 0..batch { operation(); }
    let mean = start.elapsed().as_nanos() as f64 / batch as f64;
    for _ in 0..samples { let start = Instant::now(); operation(); times.push(start.elapsed().as_nanos()); }
    let allocs = ALLOCS.load(Ordering::Relaxed) - before;
    assert_eq!(allocs, 0, "allocation in {name}"); times.sort_unstable();
    let p = |n: usize| times[(times.len() - 1) * n / 100];
    println!("{name},{size},{mean:.2},{},{},{},{:.2},{allocs},{memory}", p(50), p(95), p(99), 1e9 / mean.max(1.0));
}
fn main() {
    println!("case,input_bytes,mean_ns,p50_instrumented_ns,p95_instrumented_ns,p99_instrumented_ns,ops_per_second,total_allocations,workspace_bytes");
    for size in [64usize, 256, 1024, 4096, 65536] {
        let mut logs = "INFO repeated\nINFO repeated\nERROR keep\n".repeat(size / 10 + 1); logs.truncate(size);
        let raw = Completion { call_id: "real-call", text: &logs, is_error: false };
        let mut out = vec![0; size]; let mut a = vec![0; size]; let mut b = vec![0; size];
        measure("output_identity", size, 0, || { let result = black_box(raw).transform("build", &mut Identity, &mut []).unwrap(); black_box(result.text.as_bytes()); });
        let mut truncate = Truncate::new(size / 2, "...").unwrap();
        measure("output_truncate", size, out.len(), || { let result = black_box(raw).transform("build", &mut truncate, &mut out).unwrap(); black_box(result.text.as_bytes()); });
        measure("output_dedup", size, out.len(), || { let result = black_box(raw).transform("build", &mut DedupAdjacentLines, &mut out).unwrap(); black_box(result.text.as_bytes()); });
        let mut chain = Chain::new(Chain::new(DedupAdjacentLines, Trim, &mut a), Truncate::new(512, "\n...[truncated]").unwrap(), &mut b);
        measure("output_chain_hooks", size, out.len() + size * 2 + std::mem::size_of_val(&chain), || {
            let HookResult::Applied(result) = black_box(raw).transform_with_hooks("build", &mut chain, &mut out, &mut NoHooks).unwrap() else { panic!("unexpected pause in no-op hook benchmark"); };
            black_box(result.text.as_bytes());
        });
        let prefix = "{ \"x\" : \""; let suffix = "\" }";
        let json = format!("{prefix}{}{suffix}", "x".repeat(size - prefix.len() - suffix.len()));
        assert_eq!(json.len(), size);
        let raw = Completion { call_id: "real-json", text: &json, is_error: false };
        let mut tokens = TokenStorage::<32>::new(); let memory = out.len() + tokens.memory_bytes();
        let mut minify = JsonMinify::new(&mut tokens, Limits::default());
        measure("output_json_minify", size, memory, || { let result = black_box(raw).transform("query", &mut minify, &mut out).unwrap(); black_box(result.text.as_bytes()); });
    }
}
