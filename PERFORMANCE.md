# Performance methodology

Run `cargo bench --locked --benches` on an otherwise idle machine. Record git
revision, `rustc -Vv`, CPU, operating system and power policy. Native harness
evidence retains source, environment and three raw benchmark CSV files. CI sets
ZHARNESS_BENCH_SMOKE=1 only for correctness checks, not stable speed comparisons.

Fixture construction, allocated sample storage and printing are outside timing.
Benchmark-only System allocator wrappers count alloc, alloc_zeroed and realloc
requests and assert zero during measured operations. Unsafe forwarding is outside
the unsafe-forbidden library. User hooks/executors/payloads may allocate and must
be audited independently. Process-adapter allocations are application costs.

mean_ns is an amortized batch mean. p50/p95/p99 individually time calls and include
Instant overhead; they are not quantiles of batch means. No clock overhead is
subtracted. Throughput, where emitted, derives from mean latency. Workspace columns
count supplied buffers/descriptors, not borrowed inputs, process RSS or stack totals.

## Registry and native harness cases

The dispatch bench compares linear/indexed lookups at 1, 4, 16, 64, 256 and 1024
tools. Prepare+echo validates and copies objects at 64, 256, 1024, 4096 and 65536
bytes. Native cases cover context sliding/forking, task claim/complete/retire/
resubmit, mailbox receive/send, hook composition and loop start/cancel. The real
model/tool stream is integration-tested, not included in loop-start timing.

## Output processing cases

The output bench runs 25 cases: Identity, explicit truncation, adjacent-line
deduplication, a three-stage hooked pipeline and validated JsonMinify at 64, 256,
1024, 4096 and 65536 bytes. All copying, UTF-8 checking and JSON parsing remain
inside timing. Output slices are black-boxed to prevent dead-store elimination.
The JSON fixture is string-heavy, not a wide/deep-object or field-projection case.
No tokenizer is run and bytes saved are not asserted to equal tokens saved.

## Actual execution-hook overhead

The execution bench runs 12 cases: unhooked tool execution, tool execution with
two callbacks, command adapter execution with NoHooks, and command execution with
two callbacks, at 64, 1024 and 4096 output bytes. Gate evaluation, metadata/bounds
checks, tool UTF-8 validation, adapter buffer copies and result observations are
included. Tool preparation/schema parsing is outside timing. Hook state and output
slices are observed through black_box. The adapters are in-memory fixtures: these
numbers are NOT OS process startup, shell execution, network or real-tool latency.
No before/after speed ratio is claimed for operations with unequal validation.

The real command_hooks example verifies subprocess behavior separately, including
exit 7 and draining both pipes after capture capacity is exceeded. That example
uses standard-library threads/processes and is not a zero-allocation benchmark.

## Scaling and limits

Context insertion/eviction and mailbox removal scan/shift bounded slices. Task
lookup and dependency propagation use bounded scans. Transform chains require
caller scratch; minification/deduplication may scan multiple times. No universal
microsecond guarantee, framework speedup, cancellation timer or network-latency
claim is made. Historical reports apply only to their recorded source revisions.
