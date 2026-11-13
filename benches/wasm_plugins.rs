//! Matched Wasm calls: raw bytes, checked UTF-8, the Executor bridge, and routing.
//! Guest compilation, JSON parsing and route setup are outside steady-state timing.
//! CSV quantiles describe batch means, never individual-request tail latency.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};
use zharness::{
    Executor,
    plugin::{
        Route, Router, Slot,
        wasm::{Interface, Limits, Operation, WasmExtension},
    },
    validation::{ArgumentLimits, Arguments},
    zcall::v1::TokenStorage,
    zplugin::wasm::Plugin,
};

struct Counted;
static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static REQUESTED: AtomicU64 = AtomicU64::new(0);
static LIVE: AtomicU64 = AtomicU64::new(0);
static PEAK: AtomicU64 = AtomicU64::new(0);

fn account(size: usize) {
    ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
    REQUESTED.fetch_add(size as u64, Ordering::Relaxed);
    let live = LIVE.fetch_add(size as u64, Ordering::Relaxed) + size as u64;
    PEAK.fetch_max(live, Ordering::Relaxed);
}

// SAFETY: Allocation operations preserve System's pointers, layouts, sizes and
// zeroing. Counters inspect only requested layout sizes, never allocated memory.
unsafe impl GlobalAlloc for Counted {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            account(layout.size());
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            account(layout.size());
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size() as u64, Ordering::Relaxed);
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let replacement = unsafe { System.realloc(pointer, layout, size) };
        if !replacement.is_null() {
            LIVE.fetch_sub(layout.size() as u64, Ordering::Relaxed);
            account(size);
        }
        replacement
    }
}

#[global_allocator]
static ALLOCATOR: Counted = Counted;

const INTERFACE: Interface = Interface {
    id: 0x7a68_6172_6e65_7373,
    version: 1,
};
const OPERATIONS: [Operation<'static>; 1] = [Operation::new("echo", 0)];

struct Sample {
    nanos: u128,
    allocations: u64,
    requested: u64,
    live_delta: i128,
    peak_extra: u64,
}

fn measure(
    name: &str,
    size: usize,
    iterations: usize,
    samples: usize,
    allocation_free: bool,
    mut operation: impl FnMut(),
) {
    for _ in 0..iterations.min(64) {
        operation();
    }
    let mut rows = Vec::with_capacity(samples);
    for _ in 0..samples {
        let allocations = ALLOCATIONS.load(Ordering::Relaxed);
        let requested = REQUESTED.load(Ordering::Relaxed);
        let live = LIVE.load(Ordering::Relaxed);
        PEAK.store(live, Ordering::Relaxed);
        let start = Instant::now();
        for _ in 0..iterations {
            operation();
        }
        let nanos = start.elapsed().as_nanos();
        let row = Sample {
            nanos,
            allocations: ALLOCATIONS.load(Ordering::Relaxed) - allocations,
            requested: REQUESTED.load(Ordering::Relaxed) - requested,
            live_delta: i128::from(LIVE.load(Ordering::Relaxed)) - i128::from(live),
            peak_extra: PEAK.load(Ordering::Relaxed).saturating_sub(live),
        };
        assert_eq!(row.live_delta, 0, "{name}: net live allocation changed");
        if allocation_free {
            assert_eq!(row.allocations, 0, "{name}: warmed call allocated");
        }
        rows.push(row);
    }
    for (sample, row) in rows.iter().enumerate() {
        println!(
            "{name},{size},{},{iterations},{},{:.3},{},{},{},{}",
            sample + 1,
            row.nanos,
            row.nanos as f64 / iterations as f64,
            row.allocations,
            row.requested,
            row.live_delta,
            row.peak_extra,
        );
    }
}

fn payload(size: usize) -> Vec<u8> {
    match size {
        0 => Vec::new(),
        2 => b"{}".to_vec(),
        _ => format!("{{\"x\":\"{}\"}}", "x".repeat(size - 8)).into_bytes(),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let smoke = std::env::var_os("ZHARNESS_BENCH_SMOKE").is_some()
        || std::env::args().any(|argument| argument == "--test" || argument == "--smoke");
    let Some(path) = std::env::var_os("ZHARNESS_WASM_PLUGIN") else {
        if smoke {
            eprintln!(
                "SKIP Wasm benchmark without a guest: run python scripts/wasm_bench.py --smoke"
            );
            return Ok(());
        }
        return Err("ZHARNESS_WASM_PLUGIN is required; run python scripts/wasm_bench.py".into());
    };
    let bytes = std::fs::read(path)?;
    let limits = Limits::default();
    let samples = if smoke { 3 } else { 31 };
    let before = LIVE.load(Ordering::Relaxed);
    let mut direct = Plugin::new(&bytes, INTERFACE, limits)?;
    let direct_live = LIVE.load(Ordering::Relaxed) - before;
    let guest_memory = direct.memory_size();
    let before = LIVE.load(Ordering::Relaxed);
    let mut extension = WasmExtension::load(&bytes, INTERFACE, &OPERATIONS, limits)?;
    let extension_live = LIVE.load(Ordering::Relaxed) - before;
    println!("# statistic=batch_mean_ns_per_operation; not individual-call percentiles");
    println!("# guest_memory_bytes={guest_memory}");
    println!("# direct_retained_requested_bytes={direct_live}");
    println!("# extension_retained_requested_bytes={extension_live}");
    println!(
        "name,bytes,sample,iterations,elapsed_ns,ns_per_op,allocations,requested_bytes,net_live_bytes,peak_extra_bytes"
    );

    for size in [0, 2, 64, 4096, 1_048_576] {
        let input = payload(size);
        assert_eq!(input.len(), size);
        let mut output = vec![0; size];
        let iterations = if smoke {
            2
        } else {
            match size {
                1_048_576 => 16,
                4096 => 1000,
                _ => 10_000,
            }
        };
        assert_eq!(direct.call(0, &input, &mut output)?, size);
        assert_eq!(output, input);
        measure("direct_bytes", size, iterations, samples, true, || {
            let written = direct
                .call(black_box(0), black_box(&input), black_box(&mut output))
                .unwrap();
            black_box((written, output.as_slice()));
        });
        assert_eq!(output, input);

        assert_eq!(extension.call_bytes("echo", &input, &mut output)?, input);
        measure("extension_bytes", size, iterations, samples, true, || {
            black_box(
                extension
                    .call_bytes(black_box("echo"), black_box(&input), black_box(&mut output))
                    .unwrap(),
            );
        });
        assert_eq!(output, input);
        if size == 0 {
            continue;
        }

        // This baseline includes the same UTF-8 output validation as Executor.
        // Raw-byte calls are a separate comparison, not a cheaper substitute.
        measure("direct_text", size, iterations, samples, true, || {
            let written = direct
                .call(black_box(0), black_box(&input), black_box(&mut output))
                .unwrap();
            black_box(core::str::from_utf8(&output[..written]).unwrap());
        });
        assert_eq!(output, input);
        let arguments = Arguments::parse(
            &input,
            &mut TokenStorage::<16>::new(),
            ArgumentLimits {
                max_string_bytes: size,
                ..ArgumentLimits::default()
            },
        )?;
        let result = extension.execute("echo", arguments, &mut output)?;
        assert_eq!(result.written, size);
        assert!(!result.is_error);
        measure("executor", size, iterations, samples, true, || {
            let result = extension
                .execute(
                    black_box("echo"),
                    black_box(arguments),
                    black_box(&mut output),
                )
                .unwrap();
            black_box((result, output.as_slice()));
        });
        assert_eq!(output, input);

        let routes = [Route::new("routed_echo", 0, "echo")];
        let mut slots = [Slot::new(&mut extension)];
        let mut router = Router::new(&routes, &mut slots)?;
        let result = router.execute("routed_echo", arguments, &mut output)?;
        assert_eq!(result.written, size);
        assert!(!result.is_error);
        measure("router", size, iterations, samples, true, || {
            let result = router
                .execute(
                    black_box("routed_echo"),
                    black_box(arguments),
                    black_box(&mut output),
                )
                .unwrap();
            black_box((result, output.as_slice()));
        });
        assert_eq!(output, input);
    }

    drop(extension);
    drop(direct);
    let iterations = if smoke { 1 } else { 10 };
    measure("direct_load_drop", 0, iterations, samples, false, || {
        let plugin = Plugin::new(black_box(&bytes), INTERFACE, limits).unwrap();
        black_box(&plugin);
        drop(plugin);
    });
    measure("extension_load_drop", 0, iterations, samples, false, || {
        let plugin =
            WasmExtension::load(black_box(&bytes), INTERFACE, &OPERATIONS, limits).unwrap();
        black_box(&plugin);
        drop(plugin);
    });
    Ok(())
}
