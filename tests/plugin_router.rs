use core::cell::Cell;
use zharness::{
    Error, Executor, Invocation, Tool, ToolOutput, ToolSet,
    execution::BeforeExecute,
    hooks::{Decision, HookResult, Hooks},
    plugin::{Route, Router, Slot},
    validation::{ArgumentLimits, Arguments},
    zcall::v1::TokenStorage,
};

struct Counted<'a> {
    calls: &'a Cell<usize>,
    expected: &'a str,
    failure: Option<Error>,
    business_error: bool,
}

impl Executor for Counted<'_> {
    fn execute(
        &mut self,
        name: &str,
        arguments: Arguments<'_>,
        output: &mut [u8],
    ) -> Result<ToolOutput, Error> {
        assert_eq!(name, self.expected);
        self.calls.set(self.calls.get() + 1);
        if let Some(error) = self.failure {
            return Err(error);
        }
        let input = arguments.as_bytes();
        output
            .get_mut(..input.len())
            .ok_or(Error::Capacity {
                needed: input.len(),
            })?
            .copy_from_slice(input);
        Ok(ToolOutput {
            written: input.len(),
            is_error: self.business_error,
        })
    }
}

fn arguments() -> Arguments<'static> {
    Arguments::parse(
        br#"{"input":"unchanged"}"#,
        &mut TokenStorage::<16>::new(),
        ArgumentLimits::default(),
    )
    .unwrap()
}

fn counted<'a>(calls: &'a Cell<usize>, expected: &'a str) -> Counted<'a> {
    Counted {
        calls,
        expected,
        failure: None,
        business_error: false,
    }
}

#[test]
fn routed_names_are_explicit_and_targets_remain_independent() {
    let counts = [Cell::new(0), Cell::new(0)];
    let mut first = counted(&counts[0], "internal_a");
    let mut second = counted(&counts[1], "internal_b");
    let mut slots = [Slot::new(&mut first), Slot::new(&mut second)];
    let routes = [
        Route::new("public_a", 0, "internal_a"),
        Route::new("public_b", 1, "internal_b"),
        Route::new("alias_a", 0, "internal_a"),
    ];
    let mut router = Router::new(&routes, &mut slots).unwrap();
    assert_eq!(router.routes(), routes);
    let mut output = [0; 64];
    for name in ["public_a", "public_b", "alias_a"] {
        let result = router.execute(name, arguments(), &mut output).unwrap();
        assert_eq!(&output[..result.written], arguments().as_bytes());
        assert!(!result.is_error);
    }
    assert_eq!(counts[0].get(), 2);
    assert_eq!(counts[1].get(), 1);
}

#[test]
fn disabled_and_unknown_routes_never_touch_backend_or_output() {
    let calls = Cell::new(0);
    let mut target = counted(&calls, "echo");
    let mut slots = [Slot::new(&mut target)];
    let routes = [Route::new("visible", 0, "echo")];
    let mut router = Router::new(&routes, &mut slots).unwrap();
    let mut output = [0xa5; 64];
    assert!(router.is_enabled(0).unwrap());
    router.set_enabled(0, false).unwrap();
    assert!(!router.is_enabled(0).unwrap());
    assert_eq!(
        router
            .execute("visible", arguments(), &mut output)
            .unwrap_err(),
        Error::Denied
    );
    assert_eq!(
        router
            .execute("absent", arguments(), &mut output)
            .unwrap_err(),
        Error::UnknownTool
    );
    assert_eq!(output, [0xa5; 64]);
    assert_eq!(calls.get(), 0);
    router.set_enabled(0, true).unwrap();
    router.execute("visible", arguments(), &mut output).unwrap();
    assert_eq!(calls.get(), 1);
}

#[test]
fn replacing_a_slot_preserves_disabled_state_and_old_executor() {
    let old_calls = Cell::new(0);
    let new_calls = Cell::new(0);
    let mut old = counted(&old_calls, "echo");
    let mut new = counted(&new_calls, "echo");
    let mut slots = [Slot::new(&mut old)];
    let routes = [Route::new("visible", 0, "echo")];
    let mut router = Router::new(&routes, &mut slots).unwrap();
    router.set_enabled(0, false).unwrap();
    let returned = router.replace(0, &mut new).unwrap();
    assert!(!router.is_enabled(0).unwrap());
    let mut output = [0; 64];
    returned.execute("echo", arguments(), &mut output).unwrap();
    assert_eq!(old_calls.get(), 1);
    assert_eq!(new_calls.get(), 0);
    router.set_enabled(0, true).unwrap();
    router.execute("visible", arguments(), &mut output).unwrap();
    assert_eq!(old_calls.get(), 1);
    assert_eq!(new_calls.get(), 1);
}

#[test]
fn invalid_indices_preserve_existing_routes_and_enable_state() {
    let calls = Cell::new(0);
    let replacement_calls = Cell::new(0);
    let mut target = counted(&calls, "echo");
    let mut replacement = counted(&replacement_calls, "echo");
    let mut slots = [Slot::new(&mut target)];
    let routes = [Route::new("visible", 0, "echo")];
    let mut router = Router::new(&routes, &mut slots).unwrap();
    assert_eq!(router.set_enabled(1, false), Err(Error::InvalidRange));
    assert_eq!(router.is_enabled(1), Err(Error::InvalidRange));
    assert!(matches!(
        router.replace(1, &mut replacement),
        Err(Error::InvalidRange)
    ));
    assert!(router.is_enabled(0).unwrap());
    router
        .execute("visible", arguments(), &mut [0; 64])
        .unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(replacement_calls.get(), 0);
}

#[test]
fn configuration_rejects_duplicate_names_invalid_names_and_targets() {
    let calls = Cell::new(0);
    let mut target = counted(&calls, "echo");
    for routes in [
        [Route::new("same", 0, "echo"), Route::new("same", 0, "echo")],
        [Route::new("", 0, "echo"), Route::new("valid", 0, "echo")],
        [
            Route::new("bad name", 0, "echo"),
            Route::new("valid", 0, "echo"),
        ],
        [
            Route::new("中文", 0, "echo"),
            Route::new("valid", 0, "echo"),
        ],
        [Route::new("first", 0, ""), Route::new("valid", 0, "echo")],
        [
            Route::new("first", 0, "echo"),
            Route::new("valid", 1, "echo"),
        ],
    ] {
        let mut slots = [Slot::new(&mut target)];
        assert!(Router::new(&routes, &mut slots).is_err());
    }
    assert_eq!(calls.get(), 0);
}

#[test]
fn empty_registry_and_empty_slot_storage_are_valid_and_fail_closed() {
    let mut slots = [];
    let mut router = Router::new(&[], &mut slots).unwrap();
    let mut output = [0xa5; 8];
    assert_eq!(
        router
            .execute("anything", arguments(), &mut output)
            .unwrap_err(),
        Error::UnknownTool
    );
    assert_eq!(output, [0xa5; 8]);
}

#[test]
fn backend_failures_are_not_retried_or_rerouted() {
    let calls = Cell::new(0);
    let mut target = counted(&calls, "echo");
    target.failure = Some(Error::ExecutionFailed);
    let mut slots = [Slot::new(&mut target)];
    let routes = [Route::new("visible", 0, "echo")];
    let mut router = Router::new(&routes, &mut slots).unwrap();
    assert_eq!(
        router
            .execute("visible", arguments(), &mut [0; 64])
            .unwrap_err(),
        Error::ExecutionFailed
    );
    assert_eq!(calls.get(), 1);
}

struct Gate(Decision);
impl Hooks for Gate {
    fn before_execute(&mut self, event: BeforeExecute<'_>) -> Decision {
        assert_eq!(event.name, "public_tool");
        assert_eq!(event.call_id, "routed-1");
        self.0
    }
}

#[test]
fn public_name_is_authorized_before_mapping_and_business_status_is_preserved() {
    let calls = Cell::new(0);
    let mut target = counted(&calls, "private_operation");
    target.business_error = true;
    let mut slots = [Slot::new(&mut target)];
    let routes = [Route::new("public_tool", 0, "private_operation")];
    let mut router = Router::new(&routes, &mut slots).unwrap();
    let tools = [Tool::new(
        "public_tool",
        "Approved capability",
        r#"{"type":"object"}"#,
    )];
    let invocation = Invocation {
        signature: None,
        call_id: "routed-1",
        name: "public_tool",
        arguments: arguments().as_bytes(),
    }
    .prepare(
        &ToolSet::new(&tools).unwrap(),
        &mut TokenStorage::<16>::new(),
        ArgumentLimits::default(),
    )
    .unwrap();
    let mut output = [0xa5; 64];
    assert!(matches!(
        invocation.execute_with_hooks(&mut router, &mut output, &mut Gate(Decision::Pause)),
        Ok(HookResult::Paused)
    ));
    assert!(matches!(
        invocation.execute_with_hooks(&mut router, &mut output, &mut Gate(Decision::Deny)),
        Err(Error::Denied)
    ));
    assert_eq!(calls.get(), 0);
    assert_eq!(output, [0xa5; 64]);
    let HookResult::Applied(completion) = invocation
        .execute_with_hooks(&mut router, &mut output, &mut Gate(Decision::Allow))
        .unwrap()
    else {
        panic!("allowed route was paused")
    };
    assert!(completion.is_error);
    assert_eq!(completion.call_id, "routed-1");
    assert_eq!(calls.get(), 1);
}

#[test]
fn harness_errors_remain_bounded_copy_values() {
    fn assert_copy<T: Copy>() {}
    assert_copy::<Error>();
    #[cfg(feature = "wasm")]
    assert_copy::<zharness::plugin::wasm::Fault>();
}
