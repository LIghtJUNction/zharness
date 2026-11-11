# zharness 原生插件性能记录 — 2026-11-12

本次基准测量真实动态库已加载、接口已绑定后的调用路径。12 个测量项均完成，
宿主分配计数全部为 0；在本次 x86_64 构建中，代理值自身大小为 80 字节。
原始数据保存在 [CSV](plugins-benchmark-2026-11-12.csv)。

测量实现对应提交
[`77f70f5d4e1924f25fd61536baadc8b2de66b652`](https://github.com/LIghtJUNction/zharness/commit/77f70f5d4e1924f25fd61536baadc8b2de66b652)。
基准源码为 [benches/plugins.rs](../benches/plugins.rs)，实际加载
[native_plugin](../examples/native_plugin.rs) 动态库；接口和生命周期说明见
[PLUGINS.md](../PLUGINS.md)。

## 环境与复现

| 项目 | 本次记录 |
| --- | --- |
| Rust | 1.99.1，提交 `58cf8f9c35b355199727627d07ae34a4e2846d67` |
| LLVM | 23.1.1 |
| 目标 | `x86_64-unknown-linux-gnu` |
| 系统 | Linux 6.18.57 |
| CPU 型号 | AMD EPYC 9V74 80-Core Processor |
| 执行环境 | 共享容器，资源和调度存在波动 |
| 优化配置 | 宿主 bench / 插件 release；thin LTO、codegen-units = 1 |
| 插件依赖 | zplugin 0.1.0 |

依赖已缓存时，在仓库根目录运行本次测量命令：

```sh
cargo bench --locked --offline --bench plugins > plugin-benchmark.csv
```

首次使用可先运行 `cargo fetch --locked`。基准辅助程序会通过 Cargo 构建独立的
release 动态库，使用独立的 `target/plugin-fixtures` 目录，并从 Cargo 的 JSON
构建结果取得实际文件路径。宿主通过 zplugin 加载该文件，创建代理后开始计时；
全部测量完成后销毁代理并执行协作式关闭。

## 测量范围

每种输入大小包含四条真实调用路径：

| case | 计时内的工作 |
| --- | --- |
| `generated_call` | 缓存函数表的生成代理调用、插件 `echo`、输出拷贝 |
| `checked_bytes` | 上述调用，加返回长度和输出范围检查 |
| `prepared_executor` | 已准备调用通过 `Executor` 执行，含实际插件调用及输出验证 |
| `executor_two_hooks` | 已准备调用通过两个执行钩子和 `Executor` 执行 |

输入构造、输出缓冲区分配、JSON 参数准备、动态库构建与加载、接口绑定及实例创建
都在计时外。插件的 `echo` 实现和实际内存拷贝保留在计时内。因此表中的数字描述
重复调用成本，加载启动耗时需要另行测量。

每个 case 先预热 32 次，再对 4,096 次调用计时得到 `mean_ns`，随后采集 1,001 个
逐次计时样本计算 p50/p95/p99。百分位包含每次 `Instant` 计时的开销，未扣除时钟
成本；批量均值和逐次百分位来自不同采样方式，二者并不要求数值相等。

## 原始结果

所有时间单位均为纳秒；1,000 ns = 1 μs。百分位列对应 CSV 的
`p50_instrumented_ns`、`p95_instrumented_ns`、`p99_instrumented_ns`。

| 输入字节 | case | 批量均值 | p50 | p95 | p99 | 宿主分配次数 |
| ---: | --- | ---: | ---: | ---: | ---: | ---: |
| 64 | `generated_call` | 36.54 | 50 | 50 | 60 | 0 |
| 64 | `checked_bytes` | 39.86 | 30 | 40 | 41 | 0 |
| 64 | `prepared_executor` | 25.35 | 50 | 51 | 51 | 0 |
| 64 | `executor_two_hooks` | 45.22 | 50 | 60 | 80 | 0 |
| 1,024 | `generated_call` | 25.75 | 40 | 50 | 51 | 0 |
| 1,024 | `checked_bytes` | 25.60 | 40 | 50 | 51 | 0 |
| 1,024 | `prepared_executor` | 66.47 | 90 | 91 | 100 | 0 |
| 1,024 | `executor_two_hooks` | 75.86 | 90 | 100 | 111 | 0 |
| 4,096 | `generated_call` | 49.33 | 70 | 71 | 71 | 0 |
| 4,096 | `checked_bytes` | 49.00 | 70 | 71 | 71 | 0 |
| 4,096 | `prepared_executor` | 241.09 | 220 | 231 | 240 | 0 |
| 4,096 | `executor_two_hooks` | 237.35 | 221 | 231 | 411 | 0 |

## 分配、内存与解释边界

`host_allocations` 统计宿主基准可执行文件在整个测量区间内的 `alloc`、
`alloc_zeroed` 和 `realloc` 调用，基准逐项断言其为 0。独立动态库具有自己的分配器，
宿主计数器不覆盖任意插件内部的分配。当前示例的 `echo` 操作直接复制字节；
实际应用插件可自行使用分配器和其他资源。

`proxy_state_bytes = 80` 来自 `size_of_val(&extension)`，仅表示本次构建中代理值
自身的大小。插件对象、动态库映像、加载器状态、调用者缓冲区、其他栈空间和进程
RSS 均不包含在该数字内。

这是共享容器上的一次测量。不同 case 的微小逆序，例如某些包装路径的均值小于
生成代理调用，应按采样波动处理。此表适合记录当前实现和复现条件；稳定的性能比较
需要在相同且受控的机器上重复采样，动态库加载成本和真实插件业务耗时也应分别评估。
