# Performance methodology

Run `cargo bench --locked --bench dispatch` on an otherwise idle machine. Record
git revision, `rustc -Vv`, CPU, operating system and power policy. The Native harness
evidence workflow retains the source archive, environment and raw benchmark CSV.
CI sets ZHARNESS_BENCH_SMOKE=1 only for correctness checks; smoke numbers are not
stable performance comparisons.

The harness excludes fixture construction, allocated sample storage and printing
from timing. A benchmark-only System allocator wrapper counts alloc, alloc_zeroed
and realloc requests and asserts zero during measured operations. Its unsafe
forwarding is outside the unsafe-forbidden library. This does not certify that
arbitrary user hooks, executors, transports or payload types never allocate.

mean_ns is an amortized batch mean. p50/p95/p99 individually time calls and include
Instant instrumentation overhead; they are not quantiles of batch means. No clock
overhead is subtracted. Throughput derives from mean latency. workspace_bytes
reports the state's supplied storage (and explicitly included descriptors/indexes),
not process RSS, borrowed payloads or total thread-stack requirements.

## Cases

Registry lookups use 1, 4, 16, 64, 256 and 1024 tools, comparing linear and explicit
sorted indexes for the same last-tool hit. Prepare+echo execution validates and
copies complete objects at 64, 256, 1024, 4096 and 65536 bytes. Construction and
schema setup are excluded; a different hit/miss distribution changes crossover.

Native cases use 1, 4, 16, 64, 256 and 1024 slots: context_slide performs bounded
whole-group eviction/append; context_fork copies descriptors into preallocated
storage; task_claim_complete_requeue searches the last compatible task, completes
it, retires it and resubmits it; mailbox_receive_send shifts a FIFO and restores
its length. hook_chain_two includes an audit/guard before chain and after observers.
loop_start_cancel includes controller initialization, model-action issuance and
cancellation, not model inference or a complete remote tool turn.

## Scaling and limits

Context insertion checks duplicate IDs linearly and eviction compacts descriptors.
Fork cost is linear in supplied destination storage. Mailbox removal shifts remaining
messages. Agent/task lookup scans bounded slices; dependency failure propagation
uses bounded repeated scans and is not constant time. Large tables are intentionally
visible costs, not hidden hash allocations. Select capacities and benchmark actual
workload distributions before asserting a latency bound.

No universal microsecond guarantee, mature-framework speedup, network benchmark or
distributed exactly-once claim is made. Historical measurements in
`docs/PERFORMANCE-2026-11-11.md` describe the recorded earlier implementation only;
they are not evidence for newly added context, hooks or cluster modules.
