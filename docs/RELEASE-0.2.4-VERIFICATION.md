# zharness 0.2.4 发布验收

**zharness 0.2.4 已发布到 crates.io，GitHub 标签与正式 Release 已创建。**
发布时间为 2026-11-12 03:53:39（Asia/Taipei）。
本补丁将原生 Skills 与 zplugin 插件原子能力纳入正式包。

- [crates.io 0.2.4](https://crates.io/crates/zharness/0.2.4)
- [GitHub Release v0.2.4](https://github.com/LIghtJUNction/zharness/releases/tag/v0.2.4)
- [机器可读验收记录](release-0.2.4-verification.json)

## 安装与变更

```toml
[dependencies]
zharness = "=0.2.4"
```

默认 `std` 特性启用文件系统技能发现和原生路径加载。
需要 `no_std` 时使用 `default-features = false`。
最低 Rust 版本保持 1.99.1；直接依赖固定为 zcall 0.1.2、zplugin 0.1.0。

Skills 支持元数据解析、调用者缓冲区摘要、项目与全局目录叠加、自定义目录及
`SKILLS_PATH` 顺序控制。插件接口复用 zplugin 的真实加载器，提供版本化字节调用、
输出检查和现有 Executor/hooks 适配；操作含义、权限与调度仍由应用负责。
见 [变更日志](../CHANGELOG.md)、[Skills](../SKILLS.md) 和 [插件说明](../PLUGINS.md)。

## 发布物一致性

| 项目 | 验收结果 |
| --- | --- |
| 正式版本 | 0.2.4，未撤回 |
| 标签及包内源码提交 | `636f3b5317fbe2f909e512a2cb59cccc1053a9c2` |
| `.crate` 大小 | 136,994 字节 |
| 包内文件数 | 92 |
| 下载包 SHA-256 | `b562aedc315e60d43246a503d23118822494b0b876c94f99679de4fc884daa9c` |
| SHA-256 校验 | 与 crates.io 官方 sparse index 一致 |
| 核心内容 | Skills、plugin 源码及使用说明、宿主与动态插件示例均包含 |

包内 `.cargo_vcs_info.json` 指向发布提交，版本标签也解析到同一提交。
验收针对实际 registry 下载包；包内依赖没有 path/git 替代。

## 工作流结果

| 工作流 | 结果与证据 |
| --- | --- |
| 正式 Release | [成功](https://github.com/LIghtJUNction/zharness/actions/runs/43090363839)：Linux、Windows、macOS × stable、1.99.1 六组验证全部通过，随后完成上传、标签和 Release 创建 |
| 常规 CI | [成功](https://github.com/LIghtJUNction/zharness/actions/runs/43090363908) |
| 完整性能与源码证据 | [成功](https://github.com/LIghtJUNction/zharness/actions/runs/43090363947) |

正式流程验证测试、文档示例、Clippy、no_std、WASM、依赖审计、打包与发布预演。
实际 `cargo publish` 成功后才创建标签和 GitHub Release。

## 发布后独立消费验证

新建的独立项目只声明精确 registry 依赖 `zharness = "=0.2.4"` 和
`zplugin = "=0.1.0"`。实际下载完成后，Cargo metadata 确认两包均来自
`registry+https://github.com/rust-lang/crates.io-index`；没有 path、git 或 patch 覆盖。

验收脚本退出码为 0，完成以下检查：

- `zharness::VERSION == "0.2.4"`。
- 技能元数据、描述规范化、XML 摘要转义及正文隔离。
- 使用正式包编译真实动态库，再由独立宿主加载。
- 直接插件调用、已有工具 Executor 调用及 cooperative close。

宿主实际输出：

```text
HELLO, NATIVE EXTENSION!
native-example-1: {"message":"host-owned tool policy"}
Published registry consumer: version, skills, native load and tool execution passed
```

原生插件和宿主采用仓库中的 [native_plugin.rs](../examples/native_plugin.rs) 与
[plugin_host.rs](../examples/plugin_host.rs)。独立消费执行环境为 Linux x86_64；
Windows/macOS 的运行验证由上述正式 Release 矩阵完成。
