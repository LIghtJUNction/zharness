//! Core metadata and catalog costs; filesystem discovery and model calls are excluded.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};
use zharness::skills::{SkillMetadata, SkillSummary, write_summaries};

struct Counter;
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
// SAFETY: identical pointers, layouts and sizes are forwarded to System.
// This instrumentation belongs only to the benchmark; the library forbids unsafe.
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

fn measure(name: &str, items: usize, bytes: usize, mut operation: impl FnMut()) {
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
        "{name} allocated inside the measured region"
    );
    timings.sort_unstable();
    let percentile = |p: usize| timings[(timings.len() - 1) * p / 100];
    println!(
        "{name},{items},{bytes},{mean:.2},{},{},{},{allocations}",
        percentile(50),
        percentile(95),
        percentile(99)
    );
}

fn header(description: &str) -> String {
    let prefix = format!("---\nname: skill-bench\ndescription: {description}\n");
    let suffix = "\n---\n";
    let padding = 256 - prefix.len() - suffix.len() - 1;
    let result = format!("{prefix}#{}{suffix}", "x".repeat(padding));
    assert_eq!(result.len(), 256);
    result
}

fn main() {
    println!(
        "case,items,bytes,mean_ns,p50_instrumented_ns,p95_instrumented_ns,p99_instrumented_ns,total_allocations"
    );
    let plain = header("Inspect & report <code> changes without executing scripts.");
    let folded = header(
        ">-\n  Inspect & report <code> changes\n  without executing scripts.\n\n  Keep results actionable.",
    );
    for (kind, source) in [("plain", &plain), ("folded", &folded)] {
        let metadata = SkillMetadata::parse(source).unwrap();
        assert_eq!(metadata.body_offset(), 256);
        measure(&format!("parse_{kind}_header"), 1, source.len(), || {
            black_box(SkillMetadata::parse(black_box(source.as_str())).unwrap());
        });
        // Only the body length changes. Construct and retain it outside timing.
        let mut large = source.clone();
        large.push_str(&"b".repeat(1024 * 1024));
        assert_eq!(SkillMetadata::parse(&large).unwrap().body_offset(), 256);
        measure(&format!("parse_{kind}_large_body"), 1, large.len(), || {
            black_box(SkillMetadata::parse(black_box(large.as_str())).unwrap());
        });
        let mut output = vec![0; metadata.description().len()];
        measure(&format!("description_{kind}"), 1, output.len(), || {
            black_box(
                metadata
                    .description()
                    .write_to(black_box(&mut output))
                    .unwrap(),
            );
        });
    }

    let metadata = SkillMetadata::parse(&folded).unwrap();
    let mut description_buffer = vec![0; metadata.description().len()];
    let description = metadata
        .description()
        .write_to(&mut description_buffer)
        .unwrap();
    for count in [1, 16, 64, 256] {
        let names: Vec<_> = (0..count)
            .map(|index| format!("skill-{index:03}"))
            .collect();
        let locations: Vec<_> = names
            .iter()
            .map(|name| format!("/project/.agents/skills/{name}/SKILL.md"))
            .collect();
        let entries = || {
            names
                .iter()
                .zip(&locations)
                .map(|(name, location)| SkillSummary {
                    name,
                    description,
                    location,
                })
        };
        let mut output = vec![0; count * 512];
        let bytes = write_summaries(entries(), &mut output).unwrap().len();
        measure("catalog_folded", count, bytes, || {
            black_box(write_summaries(black_box(entries()), black_box(&mut output)).unwrap());
        });
    }
}
