# Performance methodology

Run `cargo bench --locked --benches` on an otherwise idle machine. Record git
revision, rustc -Vv, CPU, operating system and power policy. Native harness evidence
retains source, environment, test logs and four raw benchmark CSV files. CI uses
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
