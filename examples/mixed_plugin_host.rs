//! Route explicitly selected trusted native and untrusted Wasm plugins together.
use std::{env, error::Error as StdError, fs::File, io::Read};
use zharness::{
    Error, Invocation, Tool, ToolSet,
    hooks::{HookResult, NoHooks},
    plugin::{
        ExtensionApi, Plugin, Route, Router, Slot, UnloadPolicy,
        wasm::{Interface, Limits, Operation, WasmExtension},
    },
    validation::ArgumentLimits,
    zcall::v1::TokenStorage,
};

fn main() -> Result<(), Box<dyn StdError>> {
    let mut arguments = env::args_os().skip(1);
    let native_path = arguments
        .next()
        .ok_or("usage: mixed_plugin_host TRUSTED_NATIVE WASM")?;
    let wasm_path = arguments
        .next()
        .ok_or("usage: mixed_plugin_host TRUSTED_NATIVE WASM")?;
    // SAFETY: The caller supplies the reviewed native_plugin example, not an
    // untrusted file. Wasm is loaded separately and never falls back to native.
    let library = unsafe { Plugin::load_path(native_path, UnloadPolicy::Cooperative)? };
    let mut native = ExtensionApi::create(&library)?;
    let limits = Limits::default();
    let mut bytes = Vec::new();
    File::open(wasm_path)?
        .take(limits.max_module_bytes as u64 + 1)
        .read_to_end(&mut bytes)?;
    let operations = [Operation::new("echo", 0)];
    let mut wasm = WasmExtension::load(
        &bytes,
        Interface {
            id: 0x7a68_6172_6e65_7373,
            version: 1,
        },
        &operations,
        limits,
    )?;
    let routes = [
        Route::new("native_echo", 0, "echo"),
        Route::new("sandbox_echo", 1, "echo"),
    ];
    let tools = [
        Tool::new("native_echo", "Trusted native echo", r#"{"type":"object"}"#),
        Tool::new("sandbox_echo", "Sandboxed echo", r#"{"type":"object"}"#),
    ];
    let registry = ToolSet::new(&tools)?;
    {
        let mut slots = [Slot::new(&mut native), Slot::new(&mut wasm)];
        let mut router = Router::new(&routes, &mut slots)?;
        for name in ["native_echo", "sandbox_echo"] {
            let prepared = Invocation {
                signature: None,
                call_id: "mixed-example-1",
                name,
                arguments: br#"{"message":"explicit host routing"}"#,
            }
            .prepare(
                &registry,
                &mut TokenStorage::<32>::new(),
                ArgumentLimits::default(),
            )?;
            let mut output = [0; 128];
            let HookResult::Applied(result) =
                prepared.execute_with_hooks(&mut router, &mut output, &mut NoHooks)?
            else {
                unreachable!("NoHooks permits this example call")
            };
            assert_eq!(result.text.as_bytes(), prepared.arguments().as_bytes());
            println!("{name}: {}", result.text);
        }
        router.set_enabled(1, false)?;
        let prepared = Invocation {
            signature: None,
            call_id: "disabled-example",
            name: "sandbox_echo",
            arguments: b"{}",
        }
        .prepare(
            &registry,
            &mut TokenStorage::<4>::new(),
            ArgumentLimits::default(),
        )?;
        let mut untouched = [0xa5; 16];
        assert!(matches!(
            prepared.execute(&mut router, &mut untouched),
            Err(Error::Denied)
        ));
        assert_eq!(untouched, [0xa5; 16]);
    }
    drop(native);
    library.try_close().map_err(|(error, _)| error)?;
    println!("{{\"suite\":\"zharness-mixed-plugins\",\"checks\":3,\"status\":\"passed\"}}");
    Ok(())
}
