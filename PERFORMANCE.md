# Performance

## Scope

The performance target applies only to local parsing, serialization, registry
lookup and dispatch. Model inference, network transport, scheduling and user
tool execution are outside this crate's control.

## Method

Benchmarks must use identical semantic work and equivalent input/output
behavior. They must include protocol validation and report the tested tool
count and message size. A faster path that skips validation is not comparable.

Required measurements:

- MCP initialize/discover, ping and tools/list;
- tools/call parsing and result serialization;
- registry lookup at 1, 4, 16, 64, 256 and 1024 tools;
- messages of 64 B, 256 B, 1 KiB, 4 KiB and 64 KiB;
- p50, p95 and p99 latency, throughput, allocation count and peak memory.

## Evidence status

No reproducible benchmark results are published yet. The repository currently
has no benchmark harness. Microsecond-scale dispatch is a target, not a
verified performance claim.
