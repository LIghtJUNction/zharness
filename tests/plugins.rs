#![cfg(feature = "std")]

#[path = "support/plugin_build.rs"]
mod plugin_build;

use std::{fs, path::PathBuf, sync::OnceLock};
use zharness::{
    Error, Invocation, Tool, ToolSet,
    execution::{AfterExecute, BeforeExecute, ExecuteError},
    hooks::{Decision, HookResult, Hooks},
    plugin::{ExtensionApi, ExtensionProxy, Plugin, UnloadPolicy},
    validation::ArgumentLimits,
    zcall::v1::TokenStorage,
};

fn fixture() -> &'static PathBuf {
    static LIBRARY: OnceLock<PathBuf> = OnceLock::new();
    LIBRARY.get_or_init(|| plugin_build::build_example("plugin_fixture"))
}

fn calls(extension: &mut ExtensionProxy<'_>) -> u64 {
    let mut buffer = [0; 8];
    let bytes = extension.call_bytes("calls", &[], &mut buffer).unwrap();
    u64::from_le_bytes(bytes.try_into().unwrap())
}

struct Audit {
    decision: Decision,
    gates: usize,
    completions: usize,
    errors: Vec<Error>,
}
impl Audit {
    fn new(decision: Decision) -> Self {
        Self {
            decision,
            gates: 0,
            completions: 0,
            errors: Vec::new(),
        }
    }
}
impl Hooks for Audit {
    fn before_execute(&mut self, event: BeforeExecute<'_>) -> Decision {
        assert_eq!(event.call_id, "native-call-1");
        self.gates += 1;
        self.decision
    }
    fn after_execute(&mut self, event: AfterExecute<'_>) {
        assert_eq!(event.completion.call_id, "native-call-1");
        assert!(!event.completion.is_error);
        self.completions += 1;
    }
    fn execute_error(&mut self, event: ExecuteError<'_>) {
        assert_eq!(event.call.call_id, "native-call-1");
        self.errors.push(event.error);
    }
}

fn prepared(name: &'static str, arguments: &'static [u8]) -> zharness::bridge::Prepared<'static> {
    let tools = [Tool::new(name, "Fixture operation", r#"{"type":"object"}"#)];
    Invocation {
        call_id: "native-call-1",
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

#[test]
fn real_native_calls_preserve_buffers_errors_hooks_and_library_lifetimes() {
    // SAFETY: The fixture was independently built from reviewed local code; it
    // honors ABI memory/ownership and cooperative unload contracts.
    let library = unsafe { Plugin::load_path(fixture(), UnloadPolicy::Cooperative).unwrap() };
    assert_eq!(library.version().major, 1);
    assert_eq!(library.unload_policy(), UnloadPolicy::Cooperative);

    let mut specification = ExtensionApi::SPEC;
    specification.id = "dev.project-z.absent";
    assert!(matches!(
        library.bind(specification),
        Err(zplugin::Error::InterfaceNotFound)
    ));
    specification = ExtensionApi::SPEC;
    specification.version = 2;
    assert!(matches!(
        library.bind(specification),
        Err(zplugin::Error::InterfaceVersion {
            expected: 2,
            actual: 1
        })
    ));
    specification = ExtensionApi::SPEC;
    specification.signatures = &["incompatible-call"];
    assert!(matches!(
        library.bind(specification),
        Err(zplugin::Error::SignatureMismatch { method: 0 })
    ));

    let mut extension = ExtensionApi::create(&library).unwrap();
    let mut output = [0xa5; 128];
    assert_eq!(
        extension
            .call_bytes("echo", b"\0\xffbinary", &mut output)
            .unwrap(),
        b"\0\xffbinary"
    );
    assert_eq!(
        extension
            .call_text("echo", "中文🙂".as_bytes(), &mut output)
            .unwrap(),
        "中文🙂"
    );
    assert_eq!(extension.call_bytes("empty", &[], &mut []).unwrap(), b"");

    let before = calls(&mut extension);
    let mut short = [0xa5; 1];
    assert_eq!(
        extension.call_bytes("echo", b"too long", &mut short),
        Err(Error::Plugin(zplugin::Error::BufferTooSmall))
    );
    assert_eq!(calls(&mut extension), before + 1);
    assert_eq!(short, [0xa5]);

    for operation in ["overreport", "huge"] {
        assert_eq!(
            extension.call_bytes(operation, &[], &mut output),
            Err(Error::InvalidOutput)
        );
    }
    assert_eq!(
        extension
            .call_bytes("invalid_utf8", &[], &mut output)
            .unwrap(),
        &[0xff]
    );
    assert_eq!(
        extension.call_text("invalid_utf8", &[], &mut output),
        Err(Error::InvalidOutput)
    );
    assert_eq!(
        extension.call_bytes("fail", &[], &mut output),
        Err(Error::Plugin(zplugin::Error::PluginStatus(1701)))
    );

    let invocation = prepared("echo", br#"{"x":"original JSON bytes"}"#);
    let before = calls(&mut extension);
    for decision in [Decision::Pause, Decision::Deny] {
        output.fill(0xa5);
        let mut hooks = Audit::new(decision);
        let result = invocation.execute_with_hooks(&mut extension, &mut output, &mut hooks);
        match decision {
            Decision::Pause => assert!(matches!(result, Ok(HookResult::Paused))),
            Decision::Deny => assert!(matches!(result, Err(Error::Denied))),
            Decision::Allow => unreachable!(),
        }
        assert_eq!(output, [0xa5; 128]);
        assert_eq!(hooks.gates, 1);
        assert_eq!(hooks.completions, 0);
        assert!(hooks.errors.is_empty());
        assert_eq!(calls(&mut extension), before);
    }
    let mut hooks = Audit::new(Decision::Allow);
    let HookResult::Applied(completion) = invocation
        .execute_with_hooks(&mut extension, &mut output, &mut hooks)
        .unwrap()
    else {
        panic!("allowed native call was paused")
    };
    assert_eq!(
        completion.text.as_bytes(),
        invocation.arguments().as_bytes()
    );
    assert_eq!(hooks.completions, 1);
    assert!(hooks.errors.is_empty());
    assert_eq!(calls(&mut extension), before + 1);

    for (operation, expected) in [
        ("fail", Error::Plugin(zplugin::Error::PluginStatus(1701))),
        ("invalid_utf8", Error::InvalidOutput),
        ("overreport", Error::InvalidOutput),
    ] {
        let before = calls(&mut extension);
        let mut hooks = Audit::new(Decision::Allow);
        assert!(matches!(
            prepared(operation, b"{}").execute_with_hooks(&mut extension, &mut output, &mut hooks),
            Err(error) if error == expected
        ));
        assert_eq!(hooks.errors, [expected]);
        assert_eq!(hooks.completions, 0);
        assert_eq!(calls(&mut extension), before + 1);
    }

    let mut poisoned = ExtensionApi::create(&library).unwrap();
    assert_eq!(
        poisoned.call_bytes("panic", &[], &mut output),
        Err(Error::Plugin(zplugin::Error::Panicked))
    );
    assert_eq!(
        poisoned.call_bytes("echo", b"unavailable", &mut output),
        Err(Error::Plugin(zplugin::Error::Poisoned))
    );
    let mut fresh = ExtensionApi::create(&library).unwrap();
    assert_eq!(
        fresh
            .call_text("echo", b"fresh instance", &mut output)
            .unwrap(),
        "fresh instance"
    );
    drop(fresh);
    drop(poisoned);

    // The output lives in host storage and remains valid after both the object
    // and cooperative image are released.
    let retained = extension
        .call_bytes("echo", b"host-owned", &mut output)
        .unwrap();
    drop(extension);
    library.try_close().map_err(|(error, _)| error).unwrap();
    assert_eq!(retained, b"host-owned");
}

#[test]
fn unicode_library_paths_and_load_failures_use_the_real_loader() {
    let path = fixture();
    let directory = std::env::temp_dir().join(format!("zharness-插件-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let copied = directory.join(format!("插件{}", std::env::consts::DLL_SUFFIX));
    fs::copy(path, &copied).unwrap();
    // SAFETY: This is a byte-for-byte copy of the locally built trusted fixture.
    let library = unsafe { Plugin::load_path(&copied, UnloadPolicy::Cooperative).unwrap() };
    let mut extension = ExtensionApi::create(&library).unwrap();
    assert_eq!(extension.call_bytes("empty", &[], &mut []).unwrap(), b"");
    drop(extension);
    library.try_close().map_err(|(error, _)| error).unwrap();
    // SAFETY: The path intentionally does not exist, so no native code executes.
    assert!(matches!(
        unsafe { Plugin::load_path(directory.join("missing-plugin"), UnloadPolicy::Pinned) },
        Err(zplugin::Error::LoadFailed)
    ));
    fs::remove_dir_all(directory).unwrap();
}
