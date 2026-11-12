# Native plugin primitives

The `plugin` module connects zharness to zplugin's native library loader and
versioned interfaces. It provides one small byte-call contract, checked output
views and an adapter to the existing `Executor` boundary. A plugin implements
its operations; the host chooses its files, operation names, payload encoding,
tool metadata, permissions and scheduling.

The module is available in the `no_std` core. The default `std` feature forwards
to `zplugin/std` for native path handling and plugin-side panic containment.
Loading trusted native code remains an explicit unsafe host operation. Binding
and normal calls then use zplugin's generated safe proxy.

## Load, bind and call

This example loads a library whose `echo` operation returns its input. The
filename is chosen by the host; zharness does not scan plugin directories.

```rust,no_run
# #[cfg(feature = "std")]
# fn main() -> Result<(), Box<dyn std::error::Error>> {
use zharness::plugin::{ExtensionApi, Plugin, UnloadPolicy};

// SAFETY: the selected file and its dependencies are trusted native code
// implementing zplugin ABI v1 and the Extension contract below.
let library = unsafe {
    Plugin::load_path("./libnative_plugin.so", UnloadPolicy::Pinned)?
};
let mut extension = ExtensionApi::create(&library)?;
let mut output = [0; 256];
let bytes = extension.call_bytes("echo", b"hello", &mut output)?;
assert_eq!(bytes, b"hello");
let text = extension.call_text("echo", b"hello", &mut output)?;
assert_eq!(text, "hello");
# Ok(())
# }
# #[cfg(not(feature = "std"))]
# fn main() {}
```

`Plugin` and `UnloadPolicy` are the actual zplugin types re-exported by zharness.
`load_path` uses the platform's native path representation, including Unicode
Windows paths. Use `.so` on Linux, `.dylib` on macOS and `.dll` on Windows.
Without `std`, `unsafe Plugin::load(&CStr)` uses the platform loader and pins the
image. Windows also provides the allocator-free UTF-16 `load_wide` entry point.

`ExtensionApi::create` checks the interface ID, version and complete method
signature before creating an instance. The returned `ExtensionProxy` borrows
the library. Bind once and reuse it for steady calls; do not load a library or
look up a symbol for every operation.

## The interface contract

The v1 contract has ID `dev.project-z.zharness.extension` and one method:

```rust
use zharness::plugin::Extension;

struct Echo;
impl Extension for Echo {
    fn call(
        &mut self,
        operation: &str,
        input: &[u8],
        output: &mut [u8],
    ) -> Result<u64, zplugin::Error> {
        if operation != "echo" {
            return Err(zplugin::Error::Unsupported);
        }
        if output.len() < input.len() {
            return Err(zplugin::Error::BufferTooSmall);
        }
        output[..input.len()].copy_from_slice(input);
        Ok(input.len() as u64)
    }
}
```

`operation` and `input` are borrowed for one call. `output` is writable only for
that call. The success value is the number of output bytes initialized by the
plugin. The operation namespace and byte encoding belong to the application;
the crate adds no JSON envelope, tool-discovery protocol, permission convention
or result-status bit packing.

The zplugin macro generates `ExtensionApi`, `ExtensionBinding`,
`ExtensionProxy` and `ExtensionExport<T>`. `ExtensionApi::SPEC` exposes the fixed
interface specification. The interface version must change if its method name,
order, receiver or boundary types change. Plugin package versions, application
payload versions and this interface version are separate contracts.

`ExtensionProxy::call` is the generated low-level typed call and returns `u64`.
Use `call_bytes` to validate that the returned count fits `usize` and the output
buffer, then borrow exactly those bytes. `call_text` additionally validates UTF-8.
Neither helper allocates, copies the completed output or retries the plugin.

## Export a Rust plugin

The plugin is a separate `cdylib`. Depend on zharness for the shared interface
and on the same zplugin version for its export macro. Cargo unifies the zplugin
dependency; this does not create a second runtime copy inside one crate graph.

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
zharness = "=0.2.4"
zplugin = { version = "=0.1.0", default-features = false, features = ["std", "macros"] }
```

The following complete export uses the pinned lifetime policy:

```rust,no_run
# #[cfg(feature = "std")]
# mod plugin_example {
use zharness::plugin::{Extension, ExtensionExport};

struct Echo;
impl Extension for Echo {
    fn call(
        &mut self,
        operation: &str,
        input: &[u8],
        output: &mut [u8],
    ) -> Result<u64, zplugin::Error> {
        if operation != "echo" {
            return Err(zplugin::Error::Unsupported);
        }
        if output.len() < input.len() {
            return Err(zplugin::Error::BufferTooSmall);
        }
        output[..input.len()].copy_from_slice(input);
        Ok(input.len() as u64)
    }
}

fn create() -> zplugin::Result<Echo> {
    Ok(Echo)
}

zplugin::export!(ExtensionExport<Echo>,
    constructor = create, version = (1, 0, 0), unload = pinned);
# }
# fn main() {}
```

The repository's `native_plugin` example supplies `echo` and `uppercase`, and
`plugin_host` loads it through the same public interface. The example's operation
implementations are sample application logic, not behavior hidden in zharness.

```sh
cargo build --locked --example native_plugin
cargo run --locked --example plugin_host -- target/debug/examples/libnative_plugin.so
```

On macOS use `libnative_plugin.dylib`; on Windows use `native_plugin.dll`.
`python scripts/examples.py` locates Cargo's actual platform artifact and runs
the host with that path, alongside the other standalone examples.

## Use the existing tool boundary

`ExtensionProxy` implements `Executor`. A validated tool name becomes the
operation name, and the existing JSON argument bytes become the input. A valid
UTF-8 success result becomes `ToolOutput::success`. A nonzero native status stays
an execution error; application-specific business failures and their `is_error`
mapping remain the host's policy.

```rust,no_run
# #[cfg(feature = "std")]
# fn main() -> Result<(), Box<dyn std::error::Error>> {
use zharness::{Invocation, Tool, ToolSet};
use zharness::hooks::{HookResult, NoHooks};
use zharness::plugin::{ExtensionApi, Plugin, UnloadPolicy};
use zharness::validation::ArgumentLimits;
use zharness::zcall::v1::TokenStorage;

// SAFETY: this is the host-approved plugin and implements the expected ABI.
let library = unsafe {
    Plugin::load_path("./libnative_plugin.so", UnloadPolicy::Pinned)?
};
let mut extension = ExtensionApi::create(&library)?;
let tools = [Tool::new("echo", "Echo input", r#"{"type":"object"}"#)];
let registry = ToolSet::new(&tools)?;
let invocation = Invocation {
    signature: None,
    call_id: "call-1",
    name: "echo",
    arguments: br#"{"message":"hello"}"#,
};
let mut tokens = TokenStorage::<16>::new();
let prepared = invocation.prepare(&registry, &mut tokens, ArgumentLimits::default())?;
let mut output = [0; 256];
let HookResult::Applied(completion) = prepared.execute_with_hooks(
    &mut extension, &mut output, &mut NoHooks,
)? else {
    unreachable!("NoHooks always allows");
};
assert_eq!(completion.call_id, "call-1");
assert_eq!(completion.text, r#"{"message":"hello"}"#);
# Ok(())
# }
# #[cfg(not(feature = "std"))]
# fn main() {}
```

The host supplies `ToolSet` and hooks. Existing pause/deny decisions happen before
the plugin's actual call; completion/error observers keep their current timing.
Calling `call_bytes` directly intentionally bypasses that tool boundary, so the
host must place any desired policy or lifecycle wrapper around the direct call.
Nothing automatically registers plugin operations as model-visible tools.

## Errors, ownership and unloading

| Boundary | Result |
| --- | --- |
| File loading or interface binding fails | Original `zplugin::Error` |
| Native call fails | `zharness::Error::Plugin(original_error)` |
| Returned count overflows `usize` or exceeds output capacity | `zharness::Error::InvalidOutput` |
| `call_text` or executor receives invalid UTF-8 | `zharness::Error::InvalidOutput` |
| Existing execution hook pauses or denies | Plugin is not called |

An error can follow partial output writes or external side effects. Discard the
output for that failed call and decide recovery in the application. Increasing
capacity and blindly replaying an operation can repeat its effects; the adapter
never retries automatically.

Library, instance and proxy lifetimes remain zplugin-owned. Proxies are neither
`Send` nor `Sync`; calls require mutable access. The borrow prevents dropping a
library while its proxy remains usable:

```compile_fail
use zharness::plugin::{ExtensionApi, Plugin};

fn invalid_order(library: Plugin) {
    let mut extension = ExtensionApi::create(&library).unwrap();
    drop(library);
    let mut output = [0; 16];
    extension.call_bytes("echo", b"hello", &mut output).unwrap();
}
```

Pinned loading keeps the native image resident until process exit. Cooperative
unloading requires a plugin that explicitly implements zplugin's `UnloadSafe`
contract and a host that chooses `UnloadPolicy::Cooperative`. Drop all proxies
first, then call `Plugin::try_close`. A busy result returns ownership of the
library so the host can retain it and retry closing later. zharness adds no forced
unload, timer, reload watcher or migration policy.

Native code runs with the host's privileges. Metadata and length checks do not
sandbox a malicious library. Under zplugin `std` with an unwind-enabled plugin,
generated adapters contain Rust panics and poison the instance; `panic=abort`,
native faults and foreign exceptions retain their process-level behavior.

## Dependencies and performance scope

The direct runtime dependencies are the pinned Project Z crates `zcall` and
`zplugin`. zplugin's official interface macro adds `zplugin-macros`, `syn`, `quote`,
`proc-macro2` and `unicode-ident` to the compilation dependency graph. These macro
dependencies are not additional runtime libraries. The handwritten zharness crate
keeps `#![forbid(unsafe_code)]`; the generated FFI and zplugin's loader contain
their own unsafe implementation, and the host explicitly accepts native loading.

The call path uses a cached function table and caller storage. Host-side output
checking and UTF-8 validation do not allocate. Plugin initialization, a plugin's
own operation implementation and the platform loader may allocate independently.

```sh
cargo bench --locked --bench plugins
ZHARNESS_BENCH_SMOKE=1 cargo bench --locked --bench plugins
```

The benchmark builds and loads an actual native plugin before timing. For 64 B,
1 KiB and 4 KiB payloads it measures the generated call, checked byte view,
prepared executor and executor with two hooks. Fixture construction, library
loading, interface creation, JSON argument preparation and buffers are outside
timing; the plugin's echo operation and output copy remain inside it.

Each measured case asserts zero **host** `alloc`, `alloc_zeroed` and `realloc`
calls. A separately linked dynamic library has its own allocator; this counter
does not measure arbitrary plugin allocations. `proxy_state_bytes` is the Rust
proxy value's size, excluding its plugin object, library image, loader state and
caller buffer. Percentiles include per-operation clock overhead, while `mean_ns`
is a batch average. The benchmark does not measure loading latency, process RSS
or model inference, and does not promise a fixed latency on every platform.

## References

- [zplugin 0.1.0 public API](https://github.com/LIghtJUNction/zplugin/tree/v0.1.0)
- [zplugin ABI and ownership contract](https://github.com/LIghtJUNction/zplugin/blob/v0.1.0/docs/abi.md)
- [Runnable native plugin example](examples/native_plugin.rs)
- [Runnable host example](examples/plugin_host.rs)
