# Measured local performance — 2026-11-11

Source revision: `2372e9361b27e2acc0217731a5de4aa8900a5a97`.
Evidence job: https://github.com/LIghtJUNction/zharness/actions/runs/42959228105
Artifact: `zharness-evidence-2372e9361b27e2acc0217731a5de4aa8900a5a97`.
The evidence job succeeded; the independent composition jobs on this historical
revision failed due to nested Cargo workspaces. Do not interpret the artifact as
a claim that the entire historical workflow was green.

Environment: GitHub-hosted Azure x86_64 VM, 4 available virtual CPUs, host CPU
reported as AMD EPYC 9V74 80-Core Processor, Linux 7.0.0-1019-azure,
rustc 1.99.1 (58cf8f9c3 2026-10-13), LLVM 23.1.1. This is shared virtualized
hardware, not a controlled latency certification. Power policy was not controlled.

Reproduce at the recorded revision with `cargo bench --locked --bench dispatch`.
No ZHARNESS_BENCH_SMOKE environment variable was set. Fixture construction and
allocation are excluded from measured operations. The target name is the last
registered tool; different hit/miss distributions can change the crossover.

## Lookup

| Tools | Linear mean ns | Indexed mean ns | Indexed p50 ns | Indexed p95 ns | Indexed p99 ns |
|---:|---:|---:|---:|---:|---:|
| 1 | 2.73 | 4.88 | 30 | 30 | 31 |
| 4 | 10.85 | 13.56 | 50 | 51 | 51 |
| 16 | 18.98 | 28.39 | 70 | 71 | 71 |
| 64 | 150.15 | 46.99 | 90 | 100 | 101 |
| 256 | 470.12 | 69.16 | 120 | 121 | 121 |
| 1024 | 545.51 | 90.03 | 140 | 141 | 150 |

For this fixture, linear lookup is faster at 1–16 tools, while an explicit sorted
index is faster at 64–1024 tools. This supports offering both strategies rather
than forcing a hash table or claiming a universal threshold.

## Validated prepare and echo execution

| JSON bytes | Mean ns | p50 ns | p95 ns | p99 ns | Operations/s | Workspace bytes |
|---:|---:|---:|---:|---:|---:|---:|
| 64 | 91.12 | 100 | 110 | 111 | 10,974,054.51 | 704 |
| 256 | 236.80 | 280 | 291 | 291 | 4,223,033.06 | 896 |
| 1024 | 833.81 | 831 | 861 | 1041 | 1,199,317.42 | 1664 |
| 4096 | 3142.95 | 3135 | 3165 | 5748 | 318,172.08 | 4736 |
| 65536 | 50721.23 | 49934 | 55833 | 58677 | 19,715.61 | 66176 |

Every measured case reported zero allocator calls; the benchmark asserts this.
That statement covers the measured library operations and the included echo
executor, not arbitrary user tools or model/network execution. Workspace is
configured token/output storage, not process RSS or total stack consumption.

Means are amortized batch measurements. Quantiles are individually timed calls
and include clock instrumentation; they are not quantiles of batch means and no
clock overhead was subtracted. Throughput is derived from mean latency. These
values cannot be compared to another SDK without equal semantic work and inputs.
No mature MCP SDK comparison or independent hardware replication is claimed.
