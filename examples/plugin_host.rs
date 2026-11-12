//! Run with the path of the separately built native_plugin cdylib.
use std::{env, io, path::PathBuf};
use zharness::{
    Invocation, Tool, ToolSet,
    hooks::{HookResult, NoHooks},
    plugin::{ExtensionApi, Plugin, UnloadPolicy},
    validation::ArgumentLimits,
    zcall::v1::TokenStorage,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = env::args_os().nth(1).map(PathBuf::from).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "pass the native_plugin library path; python scripts/examples.py builds and runs it",
        )
    })?;
    // SAFETY: The caller supplies the trusted native_plugin example built from
    // this repository. It obeys zplugin ABI v1 and cooperative unload contracts.
    let library = unsafe { Plugin::load_path(path, UnloadPolicy::Cooperative)? };
    let mut extension = ExtensionApi::create(&library)?;
    let mut output = [0; 256];
    let text = extension.call_text("uppercase", b"Hello, native extension!", &mut output)?;
    assert_eq!(text, "HELLO, NATIVE EXTENSION!");
    println!("{text}");

    // The host explicitly exposes this tool. The library does not create a
    // registry, choose operations, or grant plugin permissions automatically.
    let tools = [Tool::new(
        "echo",
        "Echo a JSON object",
        r#"{"type":"object"}"#,
    )];
    let registry = ToolSet::new(&tools)?;
    let invocation = Invocation {
        signature: None,
        call_id: "native-example-1",
        name: "echo",
        arguments: br#"{"message":"host-owned tool policy"}"#,
    };
    let prepared = invocation.prepare(
        &registry,
        &mut TokenStorage::<32>::new(),
        ArgumentLimits::default(),
    )?;
    let HookResult::Applied(completion) =
        prepared.execute_with_hooks(&mut extension, &mut output, &mut NoHooks)?
    else {
        unreachable!("NoHooks permits this example call")
    };
    assert_eq!(completion.call_id, "native-example-1");
    assert!(!completion.is_error);
    assert_eq!(completion.text.as_bytes(), invocation.arguments);
    println!("{}: {}", completion.call_id, completion.text);

    drop(extension);
    library.try_close().map_err(|(error, _library)| error)?;
    Ok(())
}
