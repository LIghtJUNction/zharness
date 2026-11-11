# Skill performance evidence — 2026-11-11

The native skill parser and summary writer were measured with Rust 1.99.1 on
Linux 6.18.57, x86_64, AMD EPYC 9V74, in a shared container. The parser/writer
implementation is recorded in commit
[`c035d44a9ef29d1b20a9be36c57e168d13b529b8`](https://github.com/LIghtJUNction/zharness/commit/c035d44a9ef29d1b20a9be36c57e168d13b529b8).
The benchmark is `benches/skills.rs`; raw results are in
[`skills-benchmark-2026-11-11.csv`](skills-benchmark-2026-11-11.csv).

## Observed measurements

| Operation | Batch mean | Allocator calls in all measured operations |
| --- | ---: | ---: |
| Parse plain 256-byte header | 1.090 µs | 0 |
| Parse same header followed by 1 MiB body | 0.978 µs | 0 |
| Parse folded 256-byte header | 1.462 µs | 0 |
| Parse same folded header followed by 1 MiB body | 1.475 µs | 0 |
| Render plain description | 0.670 µs | 0 |
| Render folded description | 1.022 µs | 0 |
| Render catalog, 1 skill / 280 output bytes | 0.982 µs | 0 |
| Render catalog, 16 skills / 3,895 output bytes | 16.234 µs | 0 |
| Render catalog, 64 skills / 15,463 output bytes | 71.932 µs | 0 |
| Render catalog, 256 skills / 61,735 output bytes | 338.860 µs | 0 |

The equal-header pairs confirm the measured parser work stays independent of
body length: it stops at the closing frontmatter delimiter. The lower mean in
one large-body case reflects measurement variability, not a claim that larger
files parse faster. Percentiles in the CSV include per-operation clock overhead.
Fixtures, output buffers and timing storage are created outside measurement.
Catalog timing includes both preflight validation/capacity computation and writing.

These measurements cover core in-memory operations. Filesystem traversal, disk
latency, allocation of owned catalog metadata, loading instructions and model
inference are outside the benchmark. The shared host and scheduling noise limit
comparisons; the table does not define a cross-machine latency guarantee.

## Reproduction

```sh
cargo bench --locked --bench skills
```

The benchmark asserts zero `alloc`, `alloc_zeroed` and `realloc` calls while
measuring each operation. `ZHARNESS_BENCH_SMOKE=1` uses fewer iterations for CI
correctness checks. The evidence workflow also retains a full `skills-benchmark.csv`
alongside source, toolchain, CPU and test evidence for each workflow revision.
