# zharness 0.4.0 发布验收 — 2026-11-13

zharness 0.4.0 已发布到 crates.io。此次更新将 zplugin 0.3.0 的可信原生加载与
可选 Wasm 沙箱接入同一套工具执行接口，增加显式混合路由、禁用、替换和错误诊断。
正式发布工作流 9/9 个 job 成功；发布后，四个独立、仅使用 registry 依赖的项目
再次验证了新宿主、新旧原生插件和真实 Wasm guest。

本报告只将实际执行过的内容列为完成。原始环境、测试计数、包校验和、CI 产物
摘要和独立消费者依赖图保存在 [机器可读验收记录](release-0.4.0-verification.json)。

## 已完成的插件能力

| 能力 | 具体行为与边界 |
| --- | --- |
| 可信原生插件 | 保留 `Plugin`、`ExtensionApi` 和冻结的 Extension v1；加载仍要求显式接受本机代码的信任边界 |
| 不可信 Wasm | `wasm::WasmExtension::load` 接收模块字节、接口 ID/version、Limits 和宿主操作白名单；实现现有 Executor |
| 混合路由 | 调用者提供 Route/Slot 数组；Router 将公共工具名映射到明确插件及其操作名，不分配注册表或自动发现文件 |
| 启用与替换 | `set_enabled` 禁用时不触碰后端；`replace` 保持原启用状态并返回旧执行器借用；调用者负责所有权和回收 |
| 安全替换 | Wasm `reload` 先验证新实例，失败保留旧状态和诊断；成功替换不隐式迁移状态 |
| 执行授权 | 通过 Prepared 的 execution hooks 先检查公共工具名，再路由；拒绝/暂停时不调用插件，不做 fallback 或隐式重试 |
| 错误与缓冲 | 保留 Copy 的 harness 错误，在 `last_error()` 保留详细引擎诊断；检查长度和 UTF-8，使用调用者提供的输出缓冲 |

原生插件仍不能用于执行恶意本机代码。Wasm 接受的是 **zplugin 的同步、无 imports
Wasm32 协议**，不是任意 WASI 或 wasm-bindgen 程序。所有 imports 被拒绝，不授予
文件、网络、环境变量、进程或宿主回调能力；限制 guest 内存、表、调用栈、递归和
每次调用的 fuel。陷阱及预算耗尽有明确错误和实例失效行为。

Wasm 引擎、解析器、分配器和宿主仍属于可信组件。这不是不可逃逸的形式化证明；
guest 内存上限不是进程 RSS 上限，fuel 也不是模块编译的墙钟超时。需要更强的
资源或租户隔离时，应由应用另外部署进程/OS 隔离。实例会保留业务状态，不自动
清除历史输入或进行安全擦除。原生异步任务、资源句柄和回调不会自动跨接到此 Wasm ABI。

完整 API、失败语义和可运行插件/宿主示例见 [PLUGINS.md](../PLUGINS.md)。

## 依赖更新与 no_std

| 范围 | 最终版本 | 结果 |
| --- | --- | --- |
| 直接运行时依赖 | zcall 0.2.0、zplugin 0.3.0 | zplugin 从 0.1.0 升至 0.3.0；zcall 已是最新正式版 |
| 间接自研运行时 | zconv 0.3.0 | 已是最新正式版，保留精确依赖契约 |
| 仅编译期 | zplugin-macros 0.3.0、proc-macro2 1.0.107、quote 1.0.47、syn 2.0.119、unicode-ident 1.0.27 | 与运行时依赖分开审计 |
| 可选 Wasm 运行时 | wasmi / wasmi_core / wasmi_ir / wasmi_collections 2.0.0，wasmparser 0.228.0、spin 0.9.9、libm 0.2.16、bitflags 2.13.2 | 仅显式开启 wasm 时加入，共 8 个第三方运行时包 |

完整锁文件已解析到当前上游约束允许的最新兼容稳定版本。没有通过 path/Git 替换
或不兼容覆盖去强制使用 syn 3、较新 wasmparser 或 spin 0.12 版本线。
默认/原生运行时仍只有 Project Z 库；这不等于编译期也没有第三方依赖。
项目没有 dev-dependencies 或 build-dependencies。

edition 为 2024，开发编译器为 Rust 1.100.0，MSRV 保持 1.99.1。
默认 `std` 行为保持兼容；关闭默认 features 可使用 no_std core。`wasm` 通过
zplugin 启用 allocator 支持，不隐式开启 std。

```toml
# 常规宿主：原生加载 + 可选不可信 Wasm 插件
zharness = { version = "=0.4.0", features = ["wasm"] }

# no_std + alloc 宿主可改用：
# zharness = { version = "=0.4.0", default-features = false, features = ["wasm"] }
```

编译和调用示例：

```sh
rustup target add wasm32-unknown-unknown
python3 scripts/wasm.py
python3 scripts/wasm_bench.py --output artifacts/wasm-bench
```

基准命令自行构建并定位 guest，不依赖 `RUSTUP_TOOLCHAIN` 对产物目录命名的影响。

## 实际验证

所有正式候选验证均针对提交
`c33adbfc046079474b2236ba65363273777ab9ff`，不是先测旧提交再改版本发布。

| 实际环境 | Target | Rust 1.99.1 | Rust 1.100.0 |
| --- | --- | --- | --- |
| Linux 7.0.0-1019-azure / glibc 2.43 | x86_64-unknown-linux-gnu | 通过 | 通过 |
| macOS 26.7.2 ARM64 | aarch64-apple-darwin | 通过 | 通过 |
| Windows Server 2025 build 26100 | x86_64-pc-windows-msvc | 通过 | 通过 |

各任务运行 fmt、warnings-as-errors Clippy、debug/release 测试、文档测试、
严格 rustdoc、四种 feature 组合、依赖审计、真实原生/Wasm 示例及 benchmark smoke。
stable 的三个平台还验证了实际包内容。Linux stable 日志的准确计数如下：

| 配置 | 通过数 |
| --- | ---: |
| 默认 features，debug 集成测试 | 108 |
| all-features，debug 集成测试 | 115 |
| all-features，release 集成测试 | 115 |
| 文档测试 | 15 |
| Python 发布、依赖和 feature 回归测试 | 28 |
| 独立 Wasm 示例检查 | 8 |
| 原生/Wasm 混合示例检查 | 3 |
| 真实 guest 支撑的 benchmark smoke | 24 cases × 3 batches |

上述配置重复验证同一批契约，不能相加成不同测试的数量。Rust 测试主体位于
集成测试文件；lib 单元测试 harness 为 0 tests。所有正式配置均无 failed 或 ignored。
其中新增插件相关回归为 9 项 Router 测试和 7 项真实 Wasm 集成测试。

独立 portability job 验证了 core 的 wasm32/Thumb 编译，以及可选 Wasm host 的
no_std + alloc Thumb 编译；后者是编译检查，不是 Thumb 真机运行声明。还实际执行
了 Rust 1.100 guest → 1.99.1 host、1.99.1 guest → 1.100 host 两个方向的独立编译
和混合插件调用，各通过 8 项 Wasm 与 3 项混合检查。

### 发布后的 registry-only 验收

宿主、当前原生插件、旧原生插件、Wasm guest 分属四个独立 Cargo workspace，
全部第三方依赖来自 registry，无 path、Git 或 patch 覆盖。新项目锁文件中的
zharness 0.4.0 校验和均与正式发布包一致。

- 已发布 0.4.0 宿主配合 0.4.0 原生插件：10 组检查通过。
- 同一宿主配合旧版 0.3.0 / zplugin 0.1.0 原生插件：同样 10 组检查通过。
- 两组均运行真实的已发布 0.4.0 SDK 编译的 Wasm guest，覆盖真实调用、错误诊断、
  失败替换、fuel 耗尽、混合路由、拒绝 hooks、禁用和替换后旧实例存活。
- 四个项目的 fmt、Clippy 和 release 构建均通过；总验收耗时 69.549 秒，包含编译。

此消费者 guest 使用普通 Cargo 栈配置，为 1,094 B，guest 线性内存 3,211,264 B；
它与性能实验中调整过 guest 栈大小的样例不同，二者内存数字不混用。
可复现源码包、锁文件和命令记录已同步到交付目录。

## 性能结果

完整结果、原始 2,232 批记录和限制见
[Wasm 性能报告](WASM-PERFORMANCE-2026-11-13.md)。基准来自干净提交
`f90700c3cce588d263808dc47d6126c09173e09b`；与发布 tag 比较，src、测试、示例和
bench 源码未变，Cargo.toml/lock 只递增本包版本。

本次 AMD EPYC 9V74 / Linux / Rust 1.100 测量中，命名 Wasm 字节调用的代表值：

| 载荷 | zplugin 字节基线 | zharness WasmExtension |
| ---: | ---: | ---: |
| 64 B | 162.338 ns | 180.627 ns |
| 4 KiB | 360.814 ns | 335.596 ns |
| 1 MiB | 78.695 μs | 78.918 μs |

值为三个进程各自 p50 批均值的中位数，不是单次请求尾延迟。包装路径偶尔快于
基线不能解释为普遍加速。所有预热后成功 echo 调用均为零分配；初始化/销毁每次
有 122 次分配。适配器与直接路径均保留 2,247,806 B 的请求堆字节，非进程 RSS。
不将这些结果扩展到任意插件业务、错误路径或完整 agent 工作流。

## 发布、提交与交付位置

| 内容 | 位置 |
| --- | --- |
| 实现提交 | [f90700c](https://github.com/LIghtJUNction/zharness/commit/f90700c3cce588d263808dc47d6126c09173e09b) |
| 实测数据提交 | [d0a5572](https://github.com/LIghtJUNction/zharness/commit/d0a5572b6606cc7936c36b3865345816bf6c270b) |
| 只读 CI / 发布权限回归修复 | [44fe708](https://github.com/LIghtJUNction/zharness/commit/44fe7080f9df6960c6c5d76c8f2b09652c773287) |
| 精确验证并发布的版本提交 | [c33adbf](https://github.com/LIghtJUNction/zharness/commit/c33adbfc046079474b2236ba65363273777ab9ff) |
| tag / GitHub Release | [v0.4.0](https://github.com/LIghtJUNction/zharness/releases/tag/v0.4.0) |
| crates.io | [zharness 0.4.0](https://crates.io/crates/zharness/0.4.0) |
| 正式发布验证 | [Actions 43388367938](https://github.com/LIghtJUNction/zharness/actions/runs/43388367938) |
| Google Drive | [zharness-0.4.0-plugin-delivery-2026-11-13](https://drive.google.com/drive/folders/1mBDl74uIv64wt5LwNtekgq_delVjJEgm) |

发布包 SHA-256：

```text
419457ad80bad1ed19ab77d10a308c1a6389bbd21d82f8e978b0ef90758dc0d9
```

GitHub Release 创建于 2026-11-13 15:56:22（Asia/Taipei）。已验证发布流程在同一
工作流中完成计算版本、候选测试、原子提交/tag 推送、registry 包和索引校验、
Release 创建。CARGO_REGISTRY_TOKEN 仅注入需要上传的步骤。并发互斥、strict
SemVer、custom/dry_run 和不可变 tag 恢复路径均有实现及回归覆盖；未人为模拟
真实 registry 中途故障，因此不声称所有外部故障情景都已在线演练。

Drive 交付目录包含此报告、机器可读 CI/消费者证据、协议/安全文档、性能原始数据、
精确 tag 的源码包、正式 crate 包及独立消费者源码包。该报告属于发布后验收记录，
不会覆盖已发布 tag 或 registry 内容。

## 结论与表述自检

1. 需求对应：依赖更新、原生插件、Wasm 不可信执行和原子化路由均有可调用代码及示例。
2. 结论对应证据：平台、测试数、版本、包校验和和数据均由实际日志/产物支持。
3. 方法透明：区分源码 CI、精确版本候选、发布包和 registry-only 消费者四个验证阶段。
4. 安全/性能边界明确：不把 ABI 检查当隔离，不把 guest 限额或请求堆字节当 RSS。
5. 不用计划代替完成：WASI、异步 Wasm、自动迁移和进程隔离不在本次实现声明中；
   当前交付无尚未解决的权限、发布或验收阻塞。
