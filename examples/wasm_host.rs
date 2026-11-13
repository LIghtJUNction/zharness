//! Safely load a bounded Wasm file, map operation names, and use the tool boundary.
use std::{env, error::Error as StdError, fs::File, io::Read};
use zharness::{
    Error, Invocation, Tool, ToolSet,
    hooks::{HookResult, NoHooks},
    plugin::wasm::{Fault, Interface, Limits, Operation, WasmExtension},
    validation::ArgumentLimits,
    zcall::v1::TokenStorage,
};

const INTERFACE: Interface = Interface {
    id: 0x7a68_6172_6e65_7373,
    version: 1,
};
const OPERATIONS: [Operation<'static>; 8] = [
    Operation::new("echo", 0),
    Operation::new("uppercase", 1),
    Operation::new("counter", 2),
    Operation::new("fail", 3),
    Operation::new("loop", 4),
    Operation::new("trap", 5),
    Operation::new("invalid_utf8", 6),
    Operation::new("grow", 8),
];

fn main() -> Result<(), Box<dyn StdError>> {
    let path = env::args_os().nth(1).ok_or("usage: wasm_host PATH.wasm")?;
    let limits = Limits::default();
    let mut bytes = Vec::new();
    File::open(path)?
        .take(limits.max_module_bytes as u64 + 1)
        .read_to_end(&mut bytes)?;
    let mut extension = WasmExtension::load(&bytes, INTERFACE, &OPERATIONS, limits)?;
    let mut checks = 0;

    for size in [0, 64, 4096, 1_048_576] {
        let input: Vec<_> = (0..size).map(|i| (i % 251) as u8).collect();
        let mut output = vec![0xa5; size];
        assert_eq!(extension.call_bytes("echo", &input, &mut output)?, input);
    }
    checks += 1;

    let mut output = [0xa5; 128];
    assert_eq!(
        extension.call_text("uppercase", b"Hello from Wasm", &mut output)?,
        "HELLO FROM WASM"
    );
    checks += 1;

    let tools = [Tool::new("echo", "Echo JSON", r#"{"type":"object"}"#)];
    let invocation = Invocation {
        signature: None,
        call_id: "wasm-example-1",
        name: "echo",
        arguments: br#"{"message":"host-owned tool policy"}"#,
    };
    let prepared = invocation.prepare(
        &ToolSet::new(&tools)?,
        &mut TokenStorage::<32>::new(),
        ArgumentLimits::default(),
    )?;
    let HookResult::Applied(completion) =
        prepared.execute_with_hooks(&mut extension, &mut output, &mut NoHooks)?
    else {
        unreachable!("NoHooks permits this example call")
    };
    assert_eq!(completion.text.as_bytes(), invocation.arguments);
    println!("{}: {}", completion.call_id, completion.text);
    checks += 1;

    for expected in [1_u64, 2] {
        let bytes = extension.call_bytes("counter", &[], &mut output)?;
        assert_eq!(u64::from_le_bytes(bytes.try_into()?), expected);
    }
    checks += 1;

    output.fill(0xa5);
    assert_eq!(
        extension.call_bytes("fail", &[], &mut output),
        Err(Error::Wasm(Fault::GuestStatus(77)))
    );
    assert_eq!(output, [0xa5; 128]);
    assert!(!extension.is_poisoned());
    assert!(extension.last_error().is_some());
    checks += 1;

    assert!(extension.reload(b"not a Wasm binary", limits).is_err());
    let count = extension.call_bytes("counter", &[], &mut output)?;
    assert_eq!(u64::from_le_bytes(count.try_into()?), 3);
    extension.reload(&bytes, limits)?;
    let count = extension.call_bytes("counter", &[], &mut output)?;
    assert_eq!(u64::from_le_bytes(count.try_into()?), 1);
    checks += 1;

    let count = extension.call_bytes("grow", &[], &mut output)?;
    assert_eq!(u32::from_le_bytes(count.try_into()?), u32::MAX);
    output.fill(0xa5);
    assert_eq!(
        extension.call_bytes("loop", &[], &mut output),
        Err(Error::Wasm(Fault::FuelExhausted))
    );
    assert!(extension.is_poisoned());
    assert_eq!(output, [0xa5; 128]);
    checks += 1;

    extension.reload(&bytes, limits)?;
    assert!(extension.call_bytes("trap", &[], &mut output).is_err());
    assert!(extension.is_poisoned());
    extension.reload(&bytes, limits)?;
    assert_eq!(
        extension.call_text("echo", b"host alive", &mut output)?,
        "host alive"
    );
    checks += 1;
    println!("{{\"suite\":\"zharness-wasm\",\"checks\":{checks},\"status\":\"passed\"}}");
    Ok(())
}
