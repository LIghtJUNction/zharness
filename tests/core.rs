use zharness::{
    Error, Executor, Invocation, Tool, ToolOutput, ToolSet,
    bridge::InvocationStorage,
    tools::{FixedRegistry, Registry},
    validation::{ArgumentLimits, Arguments, ValidationPolicy},
    zcall::v1::*,
};

const SCHEMA: &str = r#"{"type":"object"}"#;
#[test]
fn registry_variants_and_duplicates() {
    let definitions = [Tool::new("z", "Z", SCHEMA), Tool::new("a", "A", SCHEMA)];
    let tools = ToolSet::new(&definitions).unwrap();
    let mut index = [0; 2];
    let sorted = tools.indexed(&mut index).unwrap();
    assert_eq!(
        tools.get("a").unwrap().description,
        sorted.get("a").unwrap().description
    );
    assert!(sorted.get("missing").is_none());
    assert!(tools.indexed(&mut []).is_err());
    assert!(matches!(
        ToolSet::new(&[definitions[0], definitions[0]]),
        Err(Error::DuplicateTool)
    ));
    let mut storage = [None; 1];
    let mut registry = FixedRegistry::new(&mut storage);
    let mut tokens = TokenStorage::<16>::new();
    registry.insert(definitions[0], &mut tokens).unwrap();
    assert!(registry.find("z").is_some());
    assert_eq!(
        registry.insert(definitions[0], &mut tokens),
        Err(Error::DuplicateTool)
    );
    assert_eq!(
        registry.insert(definitions[1], &mut tokens),
        Err(Error::Capacity { needed: 2 })
    );
    assert_eq!(registry.len(), 1);
}
#[test]
fn argument_validation_reuses_zcall_and_limits_scalars() {
    let mut t = TokenStorage::<64>::new();
    for input in [
        b"{".as_slice(),
        b"[]",
        b"{\"x\":1,}",
        b"{\"x\":no}",
        b"{\"x\":1}garbage",
    ] {
        assert!(Arguments::parse(input, &mut t, ArgumentLimits::default()).is_err());
    }
    assert!(matches!(
        Arguments::parse(
            br#"{"n":18446744073709551616}"#,
            &mut t,
            ArgumentLimits::default()
        ),
        Err(Error::IntegerOverflow { offset: 5 })
    ));
    Arguments::parse(
        br#"{"n":18446744073709551615,"m":-9223372036854775808}"#,
        &mut t,
        ArgumentLimits::default(),
    )
    .unwrap();
    let short = ArgumentLimits {
        max_string_bytes: 2,
        ..ArgumentLimits::default()
    };
    assert!(matches!(
        Arguments::parse(br#"{"s":"abc"}"#, &mut t, short),
        Err(Error::StringLimit { .. })
    ));
    assert!(matches!(
        Arguments::parse(br#"{"s":"\u0061\u0062\u0063"}"#, &mut t, short),
        Err(Error::StringLimit { .. })
    ));
    let limits = ArgumentLimits {
        json: Limits {
            max_depth: 1,
            ..Limits::default()
        },
        ..ArgumentLimits::default()
    };
    assert!(Arguments::parse(br#"{"a":{"b":1}}"#, &mut t, limits).is_err());
}
#[test]
fn retain_is_transactional_and_preserves_true_id() {
    let call = Invocation {
        call_id: "real-id",
        name: "echo",
        arguments: b"{}",
    };
    let mut storage = InvocationStorage::<32>::new();
    storage.store(call).unwrap();
    assert_eq!(storage.view().unwrap().call_id, "real-id");
    let large = Invocation {
        arguments: b"xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
        ..call
    };
    assert!(storage.store(large).is_err());
    assert_eq!(storage.view().unwrap().arguments, b"{}");
    storage.clear();
    assert!(storage.view().is_none());
}
struct Failing;
impl Executor for Failing {
    fn execute(
        &mut self,
        _: &str,
        _: Arguments<'_>,
        output: &mut [u8],
    ) -> Result<ToolOutput, Error> {
        output
            .get_mut(..4)
            .ok_or(Error::Capacity { needed: 4 })?
            .copy_from_slice(b"fail");
        Ok(ToolOutput::failure(4))
    }
}
struct Deny;
impl ValidationPolicy for Deny {
    fn validate(&mut self, _: &str, _: Arguments<'_>) -> Result<(), Error> {
        Err(Error::Denied)
    }
}
#[test]
fn failed_results_and_duplicate_submission_are_explicit() {
    let definitions = [Tool::new("echo", "Echo", SCHEMA)];
    let tools = ToolSet::new(&definitions).unwrap();
    let call = Invocation {
        call_id: "c1",
        name: "echo",
        arguments: b"{}",
    };
    let mut t = TokenStorage::<16>::new();
    let prepared = call
        .prepare(&tools, &mut t, ArgumentLimits::default())
        .unwrap();
    assert!(matches!(prepared.check(&mut Deny), Err(Error::Denied)));
    let mut out = [0; 16];
    let result = prepared.execute(&mut Failing, &mut out).unwrap();
    assert!(result.is_error);
    let mut records = RecordStorage::<8>::new();
    let mut builder = RequestBuilder::new(
        Protocol::AnthropicMessages.into(),
        Presence::Value(Text::new("m")),
        &mut records,
    )
    .unwrap();
    builder
        .message(Role::Assistant, &[prepared.content()])
        .unwrap();
    let mut request = builder.finish();
    result.append_to(&mut request).unwrap();
    assert!(result.append_to(&mut request).is_err());
    assert!(
        Invocation {
            name: "unknown",
            ..call
        }
        .prepare(&tools, &mut t, ArgumentLimits::default())
        .is_err()
    );
    assert!(
        Invocation {
            arguments: b"{",
            ..call
        }
        .prepare(&tools, &mut t, ArgumentLimits::default())
        .is_err()
    );
}
#[test]
fn incomplete_stream_never_produces_an_invocation() {
    let mut framing = [0; 1024];
    let mut payload = [0; 1024];
    let mut aggregate = [0; 1024];
    let mut work = StreamWorkspace::<128, 32, 8>::new();
    let mut session = Session::new(
        SessionConfig::new(Protocol::OpenAiChat.into()),
        &mut framing,
        &mut payload,
        &mut aggregate,
        &mut work,
    )
    .unwrap();
    session.prepare().unwrap();
    let full = include_bytes!("fixtures/interleaved.sse");
    let first = full.windows(2).position(|w| w == b"\n\n").unwrap() + 2;
    let mut ready = 0;
    let step = session.step(&full[..first], true, |e| {
        if Invocation::from_event(e.data()).is_some() {
            ready += 1;
        }
        Control::Continue
    });
    assert_eq!(step.consumed, first);
    assert_eq!(ready, 0);
}
#[path = "../examples/zcall_bridge.rs"]
mod bridge_example;
#[test]
fn real_interleaved_stream_and_next_turn() {
    bridge_example::main();
}
