# Performance methodology

Run `cargo bench --locked --bench dispatch` on an otherwise idle machine. Record
`rustc -Vv`, CPU model, operating system, power policy and git revision alongside
stdout. CI sets ZHARNESS_BENCH_SMOKE=1 only to detect breakage; smoke output is not
a stable performance comparison across machines.

The benchmark compares linear and sorted-index lookups at 1, 4, 16, 64, 256 and
1024 tools using identical successful target names. Construction and fixture
allocation occur outside timing. Prepare+execute validates an object and copies
its bytes through a user executor at 64, 256, 1024, 4096 and 65536 bytes.

mean_ns is amortized over a batch. The p50/p95/p99 columns measure individual calls
and include Instant instrumentation overhead; they are not quantiles of batch
means. No clock overhead is subtracted. Throughput is derived from the batch mean.
A benchmark-only counting System allocator asserts zero allocations during the
measured operations. Its unsafe delegation is isolated from the unsafe-forbidden
library. workspace_bytes reports descriptors/indexes or validation/output storage,
not process RSS, thread stack reservations, allocator metadata or fixture strings.

These are local component measurements, not network/model latency promises. The
full zcall stream bridge is integration-tested; no fabricated standalone ToolReady
latency or mature-MCP-comparison speedup is claimed. A fair independent SDK
comparison still requires equal protocol validation, capabilities and output.
