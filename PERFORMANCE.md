# Performance methodology

Run `cargo bench --locked --benches` on an otherwise idle machine. Record git
revision, rustc -Vv, CPU, operating system and power policy. Native harness evidence
retains source, environment, test logs and six raw benchmark CSV files. CI uses
ZHARNESS_BENCH_SMOKE=1 for correctness checks only; the full evidence step unsets it.

Fixture construction, sample vectors and printing are outside timing. Benchmark-
only System allocator wrappers count alloc, alloc_zeroed and realloc and assert
zero calls during measurement. Their unsafe forwarding is not linked into the
unsafe-forbidden library. Arbitrary user hooks, payloads and adapters may allocate
independently. Real OS process and transport costs are not disguised as core costs.

mean_ns is a batch-amortized mean. p50/p95/p99 time individual operations including
Instant overhead; they are not quantiles of batch means. Clock overhead is not
subtracted. Workspace columns report configured state/buffers, not process RSS,
borrowed payloads or all stack frames. No-hook controls still have explicit state
validation/counter work; no universal zero-overhead claim is made.

## Existing suites

The 43 dispatch cases cover linear/indexed registries, validated echo execution,
context sliding/forks, task claim/complete/requeue, mailboxes, hooks and loop starts.
The 25 output cases cover identity, marked UTF-8 truncation, adjacent-line dedup,
a three-stage pipeline and validated JSON minification at 64 B through 64 KiB.
The 12 execution cases compare local tool/command adapter wrappers with callback
variants. They do not measure command process startup or external tool latency.

## Typed lifecycle suite (published in 0.2.3)

Thirteen cases cover no-hook paired operations, two gates/observers, split receipt
completion, committed model switching, and conversation acceptance, memory sending
and normalized response-event handling at 64, 1024 and 4096 bytes. Gate/observer
calls, validation, redaction, state transitions and required memory copies remain
inside the measurement. Callback state, output slices and operation results are
black-boxed to avoid removing the measured work. Fixtures are prepared beforehand.

The response case consumes already normalized Started/Delta/Terminal events; it
does not include JSON/SSE parsing or model inference. Real zcall parsing and paused
event replay are separately integration-tested. Conversation cases accept a borrowed
payload, not a database transaction. Model switching updates a local configuration,
not a network route or provider session. Memory Sender is not HTTP latency.

Table scans, context/mailer shifts and dependency failure propagation remain
bounded but size-dependent costs. Transform pipelines can require multiple scans
and caller scratch. Event payload length computation may decode escaped text.
No framework speedup, tokenizer savings, cancellation timer or universal latency
guarantee is asserted. Historical reports apply only to their recorded revision.

## Skill metadata and catalogs

`cargo bench --locked --bench skills` measures borrowed parsing, normalized
description rendering and complete escaped catalogs with 1, 16, 64 or 256 entries.
Each parser fixture pairs a 256-byte header with the identical header followed
by a 1 MiB body. Parsing stops at frontmatter; instruction size does not add a
body scan. Catalog timing includes validation/capacity preflight and output.
All measured operations assert zero allocator calls. Filesystem discovery and
activation allocate separately and are excluded from these core measurements.
See [SKILLS.md](SKILLS.md) for reproduction and functional boundaries.

## Native plugins

`cargo bench --locked --bench plugins` builds and loads a real local dynamic
library, then measures cached proxy calls, checked byte calls and the Executor
bridge with and without execution hooks. Fixtures, compilation, loading, binding
and object creation remain outside timing. The host allocator counter asserts
zero measured host allocations; an independently linked plugin has its own
allocator and is outside that counter. No startup-time or arbitrary plugin
allocation claim is inferred from steady calls. The sample operation copies bytes;
payload size and application work remain part of the measured call cost.
