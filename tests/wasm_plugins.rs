#![cfg(all(feature = "std", feature = "wasm"))]

#[path = "support/plugin_build.rs"]
mod plugin_build;
#[path = "support/wasm_build.rs"]
mod wasm_build;

use zharness::{
    Error, Invocation, Tool, ToolSet,
    bridge::Prepared,
    execution::{AfterExecute, BeforeExecute, ExecuteError},
    hooks::{Decision, HookResult, Hooks},
    plugin::{
        ExtensionApi, Plugin, Route, Router, Slot, UnloadPolicy,
        wasm::{Fault, Interface, Limits, LoadError, Operation, SandboxError, WasmExtension},
    },
    validation::ArgumentLimits,
    zcall::v1::TokenStorage,
};

const INTERFACE: Interface = Interface {
    id: 0x7a68_6172_6e65_7373,
    version: 1,
};
const OPERATIONS: [Operation<'static>; 10] = [
    Operation::new("echo", 0),
    Operation::new("uppercase", 1),
    Operation::new("counter", 2),
    Operation::new("fail", 3),
    Operation::new("loop", 4),
    Operation::new("trap", 5),
    Operation::new("invalid_utf8", 6),
    Operation::new("calls", 7),
    Operation::new("grow", 8),
    Operation::new("unsupported", u32::MAX),
];

fn load_plugin() -> WasmExtension<'static> {
    WasmExtension::load(
        wasm_build::guest(),
        INTERFACE,
        &OPERATIONS,
        Limits::default(),
    )
    .unwrap()
}

fn number(plugin: &mut WasmExtension<'_>, operation: &str) -> u64 {
    let mut output = [0; 8];
    u64::from_le_bytes(
        plugin
            .call_bytes(operation, &[], &mut output)
            .unwrap()
            .try_into()
            .unwrap(),
    )
}

fn prepared(name: &'static str, arguments: &'static [u8]) -> Prepared<'static> {
    let tools = [Tool::new(
        name,
        "Explicit test operation",
        r#"{"type":"object"}"#,
    )];
    Invocation {
        signature: None,
        call_id: "wasm-call",
        name,
        arguments,
    }
    .prepare(
        &ToolSet::new(&tools).unwrap(),
        &mut TokenStorage::<32>::new(),
        ArgumentLimits::default(),
    )
    .unwrap()
}

struct Audit {
    decision: Decision,
    gates: usize,
    completed: usize,
    errors: Vec<Error>,
}
impl Audit {
    fn new(decision: Decision) -> Self {
        Self {
            decision,
            gates: 0,
            completed: 0,
            errors: Vec::new(),
        }
    }
}
impl Hooks for Audit {
    fn before_execute(&mut self, event: BeforeExecute<'_>) -> Decision {
        assert_eq!(event.call_id, "wasm-call");
        self.gates += 1;
        self.decision
    }
    fn after_execute(&mut self, event: AfterExecute<'_>) {
        assert_eq!(event.completion.call_id, "wasm-call");
        self.completed += 1;
    }
    fn execute_error(&mut self, event: ExecuteError<'_>) {
        self.errors.push(event.error);
    }
}

#[test]
fn named_calls_validate_lengths_utf8_and_retain_engine_diagnostics() {
    let mut plugin = load_plugin();
    assert_eq!(plugin.interface(), INTERFACE);
    assert_eq!(plugin.operations(), OPERATIONS);
    assert_eq!(plugin.runtime().input_capacity(), 1_048_576);
    for size in [0, 64, 4096, 1_048_576] {
        let input: Vec<_> = (0..size).map(|i| (i % 251) as u8).collect();
        let mut output = vec![0xa5; size];
        assert_eq!(
            plugin.call_bytes("echo", &input, &mut output).unwrap(),
            input
        );
    }
    let mut output = [0xa5; 32];
    assert_eq!(
        plugin
            .call_text("echo", "中文🙂".as_bytes(), &mut output)
            .unwrap(),
        "中文🙂"
    );
    assert_eq!(
        plugin.call_bytes("fail", &[], &mut output),
        Err(Error::Wasm(Fault::GuestStatus(77)))
    );
    assert!(matches!(
        plugin.last_error(),
        Some(SandboxError::GuestStatus(77))
    ));
    output.fill(0xa5);
    let before = number(&mut plugin, "calls");
    assert_eq!(
        plugin.call_bytes("hidden", &[], &mut output),
        Err(Error::UnknownTool)
    );
    assert!(plugin.last_error().is_none());
    assert_eq!(number(&mut plugin, "calls"), before);
    assert_eq!(output, [0xa5; 32]);
    assert_eq!(
        plugin.call_bytes("echo", b"too long", &mut output[..1]),
        Err(Error::Wasm(Fault::GuestStatus(-3)))
    );
    assert_eq!(output, [0xa5; 32]);
    assert_eq!(
        plugin.call_bytes("unsupported", &[], &mut output),
        Err(Error::Wasm(Fault::GuestStatus(-7)))
    );
    assert_eq!(
        plugin.call_text("invalid_utf8", &[], &mut output),
        Err(Error::InvalidOutput)
    );
    assert_eq!(output[0], 0xff);
    assert!(plugin.last_error().is_none());
    assert!(!plugin.is_poisoned());
    assert_eq!(
        plugin.call_bytes("echo", &vec![0; 1_048_577], &mut output),
        Err(Error::Wasm(Fault::InputTooLarge {
            size: 1_048_577,
            limit: 1_048_576
        }))
    );
    assert!(matches!(
        plugin.last_error(),
        Some(SandboxError::InputTooLarge { .. })
    ));
    assert_eq!(
        plugin.call_bytes("echo", b"ok", &mut output).unwrap(),
        b"ok"
    );
    assert!(plugin.last_error().is_none());
}

#[test]
fn host_allowlist_is_validated_before_module_parsing() {
    for name in ["", "has space", "中文", "control\n"] {
        let operations = [Operation::new(name, 0)];
        assert!(matches!(
            WasmExtension::load(b"invalid", INTERFACE, &operations, Limits::default()),
            Err(LoadError::Configuration(Error::InvalidTool))
        ));
    }
    let duplicate = [Operation::new("echo", 0), Operation::new("echo", 1)];
    assert!(matches!(
        WasmExtension::load(b"invalid", INTERFACE, &duplicate, Limits::default()),
        Err(LoadError::Configuration(Error::DuplicateTool))
    ));
    // Explicit aliases are allowed; no operations are automatically exposed.
    let aliases = [Operation::new("a", 0), Operation::new("b", 0)];
    let mut plugin =
        WasmExtension::load(wasm_build::guest(), INTERFACE, &aliases, Limits::default()).unwrap();
    let mut output = [0; 8];
    assert_eq!(
        plugin.call_bytes("a", b"first", &mut output).unwrap(),
        b"first"
    );
    assert_eq!(
        plugin.call_bytes("b", b"second", &mut output).unwrap(),
        b"second"
    );
    assert_eq!(
        plugin.call_bytes("echo", b"hidden", &mut output),
        Err(Error::UnknownTool)
    );
}

#[test]
fn malformed_imported_and_incompatible_modules_are_rejected() {
    assert!(matches!(
        WasmExtension::load(
            b"not native fallback",
            INTERFACE,
            &OPERATIONS,
            Limits::default()
        ),
        Err(LoadError::Sandbox(SandboxError::InvalidModule(_)))
    ));
    // A valid module importing env.noop: no host capability is provided.
    let imported = b"\0asm\x01\0\0\0\x01\x04\x01\x60\0\0\x02\x0c\x01\x03env\x04noop\0\0";
    assert!(matches!(
        WasmExtension::load(imported, INTERFACE, &OPERATIONS, Limits::default()),
        Err(LoadError::Sandbox(SandboxError::ImportsDenied))
    ));
    for interface in [
        Interface {
            id: INTERFACE.id + 1,
            version: 1,
        },
        Interface {
            id: INTERFACE.id,
            version: 2,
        },
    ] {
        assert!(matches!(
            WasmExtension::load(
                wasm_build::guest(),
                interface,
                &OPERATIONS,
                Limits::default()
            ),
            Err(LoadError::Sandbox(
                SandboxError::InterfaceId { .. } | SandboxError::InterfaceVersion { .. }
            ))
        ));
    }
    let limits = Limits {
        max_module_bytes: 8,
        ..Limits::default()
    };
    assert!(matches!(
        WasmExtension::load(wasm_build::guest(), INTERFACE, &OPERATIONS, limits),
        Err(LoadError::Sandbox(SandboxError::ModuleTooLarge { .. }))
    ));
}

#[test]
fn execution_budgets_traps_and_growth_cannot_escape_the_sandbox() {
    let mut plugin = load_plugin();
    let initial_memory = plugin.runtime().memory_size();
    let mut output = [0xa5; 16];
    let result = plugin.call_bytes("grow", &[], &mut output).unwrap();
    assert_eq!(u32::from_le_bytes(result.try_into().unwrap()), u32::MAX);
    assert_eq!(plugin.runtime().memory_size(), initial_memory);
    plugin
        .reload(
            wasm_build::guest(),
            Limits {
                fuel_per_call: 1000,
                ..Limits::default()
            },
        )
        .unwrap();
    output.fill(0xa5);
    assert_eq!(
        plugin.call_bytes("loop", &[], &mut output),
        Err(Error::Wasm(Fault::FuelExhausted))
    );
    assert!(matches!(
        plugin.last_error(),
        Some(SandboxError::FuelExhausted)
    ));
    assert!(plugin.is_poisoned());
    assert_eq!(
        plugin.call_bytes("echo", b"no", &mut output),
        Err(Error::Wasm(Fault::Poisoned))
    );
    assert_eq!(output, [0xa5; 16]);
    plugin
        .reload(wasm_build::guest(), Limits::default())
        .unwrap();
    assert_eq!(
        plugin.call_bytes("trap", &[], &mut output),
        Err(Error::Wasm(Fault::Trap))
    );
    assert!(plugin.is_poisoned());
    assert_eq!(output, [0xa5; 16]);
    assert!(matches!(plugin.last_error(), Some(SandboxError::Trap(_))));
}

#[test]
fn failed_reload_preserves_state_and_successful_reload_resets_it() {
    let mut plugin = load_plugin();
    assert_eq!(number(&mut plugin, "counter"), 1);
    let mut output = [0; 8];
    assert!(plugin.call_bytes("fail", &[], &mut output).is_err());
    assert!(
        plugin
            .reload(b"invalid replacement", Limits::default())
            .is_err()
    );
    assert!(matches!(
        plugin.last_error(),
        Some(SandboxError::GuestStatus(77))
    ));
    assert_eq!(number(&mut plugin, "counter"), 2);
    assert!(
        plugin
            .reload(
                wasm_build::guest(),
                Limits {
                    max_memory_bytes: 65_536,
                    ..Limits::default()
                }
            )
            .is_err()
    );
    assert_eq!(number(&mut plugin, "counter"), 3);
    let mut independent = load_plugin();
    assert_eq!(number(&mut independent, "counter"), 1);
    plugin
        .reload(wasm_build::guest(), Limits::default())
        .unwrap();
    assert!(plugin.last_error().is_none());
    assert_eq!(number(&mut plugin, "counter"), 1);
    let retained = plugin.call_bytes("echo", b"owned", &mut output).unwrap();
    drop(plugin);
    assert_eq!(retained, b"owned");
}

#[test]
fn hooks_gate_actual_execution_and_observe_exactly_one_result() {
    let mut plugin = load_plugin();
    let invocation = prepared("echo", br#"{"message":"unchanged bytes"}"#);
    let mut output = [0xa5; 128];
    for decision in [Decision::Pause, Decision::Deny] {
        let mut hooks = Audit::new(decision);
        let result = invocation.execute_with_hooks(&mut plugin, &mut output, &mut hooks);
        match decision {
            Decision::Pause => assert!(matches!(result, Ok(HookResult::Paused))),
            Decision::Deny => assert!(matches!(result, Err(Error::Denied))),
            Decision::Allow => unreachable!(),
        }
        assert_eq!(hooks.gates, 1);
        assert_eq!(hooks.completed, 0);
        assert!(hooks.errors.is_empty());
        assert_eq!(number(&mut plugin, "calls"), 0);
        assert_eq!(output, [0xa5; 128]);
    }
    let mut hooks = Audit::new(Decision::Allow);
    let HookResult::Applied(result) = invocation
        .execute_with_hooks(&mut plugin, &mut output, &mut hooks)
        .unwrap()
    else {
        panic!("allowed Wasm call paused")
    };
    assert_eq!(result.text.as_bytes(), invocation.arguments().as_bytes());
    assert_eq!(hooks.completed, 1);
    assert_eq!(number(&mut plugin, "calls"), 1);
    for (name, error) in [
        ("fail", Error::Wasm(Fault::GuestStatus(77))),
        ("invalid_utf8", Error::InvalidOutput),
    ] {
        let mut hooks = Audit::new(Decision::Allow);
        assert!(
            matches!(prepared(name, b"{}").execute_with_hooks(&mut plugin, &mut output, &mut hooks), Err(actual) if actual == error)
        );
        assert_eq!(hooks.errors, [error]);
        assert_eq!(hooks.completed, 0);
    }
    assert_eq!(number(&mut plugin, "calls"), 3);
}

#[test]
fn mixed_router_uses_explicit_slots_and_preserves_replaced_instances() {
    let path = plugin_build::build_example("plugin_fixture");
    // SAFETY: This is the trusted, independently built local fixture, not the Wasm file.
    let library = unsafe { Plugin::load_path(path, UnloadPolicy::Cooperative).unwrap() };
    let mut native = ExtensionApi::create(&library).unwrap();
    let mut wasm = load_plugin();
    let mut replacement = load_plugin();
    let routes = [
        Route::new("native_echo", 0, "echo"),
        Route::new("sandbox_echo", 1, "echo"),
    ];
    let mut output = [0xa5; 64];
    {
        let mut slots = [Slot::new(&mut native), Slot::new(&mut wasm)];
        let mut router = Router::new(&routes, &mut slots).unwrap();
        for name in ["native_echo", "sandbox_echo"] {
            let result = prepared(name, b"{\"x\":1}")
                .execute(&mut router, &mut output)
                .unwrap();
            assert_eq!(result.text, "{\"x\":1}");
        }
        router.set_enabled(1, false).unwrap();
        output.fill(0xa5);
        assert!(matches!(
            prepared("sandbox_echo", b"{}").execute(&mut router, &mut output),
            Err(Error::Denied)
        ));
        assert_eq!(output, [0xa5; 64]);
        let old = router.replace(1, &mut replacement).unwrap();
        assert_eq!(
            old.execute("echo", prepared("echo", b"{}").arguments(), &mut output)
                .unwrap()
                .written,
            2
        );
        assert!(!router.is_enabled(1).unwrap());
        router.set_enabled(1, true).unwrap();
        assert_eq!(
            prepared("sandbox_echo", b"{}")
                .execute(&mut router, &mut output)
                .unwrap()
                .text,
            "{}"
        );
    }
    assert_eq!(number(&mut wasm, "calls"), 2);
    assert_eq!(number(&mut replacement, "calls"), 1);
    drop(native);
    library.try_close().map_err(|(error, _)| error).unwrap();
    // The native image's lifetime is independent of the surviving Wasm instance.
    assert_eq!(
        wasm.call_bytes("echo", b"still alive", &mut output)
            .unwrap(),
        b"still alive"
    );
}
