# Performance methodology

Run `cargo bench --locked --benches` on an otherwise idle machine. Record git
revision, `rustc -Vv`, CPU, operating system and power policy. Native harness
evidence retains source, environment, benchmark.csv and output-benchmark.csv.
CI sets ZHARNESS_BENCH_SMOKE=1 only for correctness checks; smoke numbers are not
stable performance comparisons.

Fixture construction, allocated sample storage and printing are outside timing.
Benchmark-only System allocator wrappers count alloc, alloc_zeroed and realloc
requests and assert zero during measured operations. Unsafe forwarding is outside
the unsafe-forbidden library. Arbitrary user hooks/executors/payload types may
allocate independently and must be audited separately.

mean_ns is an amortized batch mean. p50/p95/p99 individually time calls and include
Instant overhead; they are not quantiles of batch means. No clock overhead is
subtracted. Throughput derives from mean latency. workspace_bytes counts supplied
buffers/descriptors for each case, not borrowed inputs, process RSS or stack totals.

## Registry and native harness cases

The dispatch bench compares linear/indexed lookups at 1, 4, 16, 64, 256 and 1024
tools. Prepare+echo validates and copies objects at 64, 256, 1024, 4096 and 65536
bytes. Native cases cover context sliding/forking, task claim/complete/retire/
resubmit, mailbox receive/send, hook composition and loop start/cancel. The real
model/tool stream is integration-tested, not included in the loop-start timing.

## Output processing cases

The output bench runs 25 cases: Identity, explicit truncation, adjacent-line
deduplication, a three-stage hooked pipeline and validated JsonMinify, each at
64, 256, 1024, 4096 and 65536 input bytes. Data is generated outside timing and
exact input sizes are checked. Pipelines keep intermediate/target buffers outside
timing but all copying, UTF-8 checking, JSON parsing and no-op result-hook dispatch
remain inside the measured operation. Output slices are black-boxed to prevent
dead-store elimination. Identity intentionally returns the original borrowed text.

The JSON fixture is string-heavy and removes whitespace around one property; it
is not a representative benchmark of wide/deep objects or schema-aware filtering.
The pipeline includes DedupAdjacentLines, Trim and a 512-byte Truncate with marker.
Lossy log reduction saves fixture bytes; no tokenizer is run and no universal
claim about tokens saved or semantic information retention is made.

## Scaling and limits

Context insertion/eviction and mailbox removal scan/shift bounded slices. Task
lookup and dependency propagation use bounded scans. Transform chains need
caller scratch for each intermediate; they do not hide a growing buffer. Lossless
minification and deduplication validate/count before emitting and therefore scan
input more than once. These costs remain in the benchmark, not skipped for speed.

No universal microsecond guarantee, mature-framework speedup or network/model
latency claim is made. Historical measurements in docs/PERFORMANCE-2026-11-11.md
refer only to their recorded earlier implementation, not these added transforms.
