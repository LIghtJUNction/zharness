use zharness::{Error, ToolResultTransform, bridge::Completion, hooks::{AfterResult, BeforeResult, Decision, HookResult, Hooks, NoHooks}, output::{Chain, DedupAdjacentLines, Identity, JsonMinify, ResultMeta, Transform, Trim, Truncate}, zcall::v1::{Json, Limits, TokenStorage}};

fn original(text: &str) -> Completion<'static, '_> { Completion { call_id: "real-call-7", text, is_error: true } }

#[test]
fn no_op_is_zero_copy_even_without_output_storage() {
    let raw = original("error: 中文");
    let compact = raw.transform("build", &mut Identity, &mut []).unwrap();
    assert_eq!(compact.text.as_ptr(), raw.text.as_ptr());
    assert_eq!(compact.call_id, raw.call_id); assert!(compact.is_error);
    assert_eq!(original("").transform("build", &mut Identity, &mut []).unwrap().text, "");
}
#[test]
fn trimming_and_adjacent_dedup_are_explicit_and_bounded() {
    let mut out = [0; 128];
    assert_eq!(original(" \n中文\t ").transform("log", &mut Trim, &mut out).unwrap().text, "中文");
    assert_eq!(original(" \t\n").transform("log", &mut Trim, &mut []).unwrap().text, "");
    let input = "a\r\na\r\nb\nb\na\nlast";
    assert_eq!(original(input).transform("log", &mut DedupAdjacentLines, &mut out).unwrap().text, "a\r\nb\na\nlast");
    assert_eq!(original("a\na").transform("log", &mut DedupAdjacentLines, &mut []).unwrap().text, "a\na");
    let raw = original(" abc ");
    assert!(matches!(raw.transform("log", &mut Trim, &mut [0; 2]), Err(Error::Capacity { needed: 3 })));
    assert_eq!(raw.text, " abc ");
}
#[test]
fn truncation_keeps_utf8_and_includes_marker_in_byte_budget() {
    let input = "中文🙂abc中文🙂";
    for budget in 3..=input.len() + 2 {
        let mut out = [0; 128];
        let got = original(input).transform("log", &mut Truncate::new(budget, "...").unwrap(), &mut out).unwrap();
        assert!(got.text.len() <= budget);
        if budget < input.len() { assert!(got.text.ends_with("...")); assert!(input.starts_with(&got.text[..got.text.len() - 3])); }
        else { assert_eq!(got.text, input); }
    }
    assert!(Truncate::new(2, "...").is_err()); assert!(Truncate::new(9, "").is_err());
    let mut out = [0; 3];
    assert_eq!(original("abcdef").transform("log", &mut Truncate::new(3, "...").unwrap(), &mut out).unwrap().text, "...");
    assert!(matches!(original("abcdef").transform("log", &mut Truncate::new(4, "...").unwrap(), &mut []), Err(Error::Capacity { needed: 4 })));
}
#[test]
fn json_minification_validates_and_preserves_lexemes_and_escapes() {
    let input = r#" { "text" : " a  b\t\"\\ ", "n" : -1.20e+03, "x" : [true, null, "\u4e2d"] } "#;
    let expected = r#"{"text":" a  b\t\"\\ ","n":-1.20e+03,"x":[true,null,"\u4e2d"]}"#;
    let mut tokens = TokenStorage::<64>::new(); let mut out = [0; 512];
    let got = original(input).transform("json", &mut JsonMinify::new(&mut tokens, Limits::default()), &mut out).unwrap();
    assert_eq!(got.text, expected); assert!(got.is_error);
    Json::parse(got.text.as_bytes(), &mut tokens, Limits::default()).unwrap();
    let mut tokens = TokenStorage::<64>::new();
    assert_eq!(original(expected).transform("json", &mut JsonMinify::new(&mut tokens, Limits::default()), &mut []).unwrap().text, expected);
    for malformed in ["", "{", "{\"x\":1,}", "{\"x\":NaN}", "true false"] {
        assert!(original(malformed).transform("json", &mut JsonMinify::new(&mut tokens, Limits::default()), &mut out).is_err());
    }
    assert!(original(input).transform("json", &mut JsonMinify::new(&mut TokenStorage::<1>::new(), Limits::default()), &mut out).is_err());
    let limits = Limits { max_input: 2, ..Limits::default() };
    assert!(original(input).transform("json", &mut JsonMinify::new(&mut tokens, limits), &mut out).is_err());
}
struct Bad { calls: usize, invalid_utf8: bool }
impl ToolResultTransform for Bad {
    fn transform(&mut self, _: ResultMeta<'_>, _: &str, output: &mut [u8]) -> Result<Transform, Error> {
        self.calls += 1;
        if self.invalid_utf8 { output[0] = 0xff; Ok(Transform::Written(1)) } else { Ok(Transform::Written(usize::MAX)) }
    }
}
struct Counter(usize);
impl ToolResultTransform for Counter {
    fn transform(&mut self, meta: ResultMeta<'_>, _: &str, _: &mut [u8]) -> Result<Transform, Error> {
        assert_eq!(meta.tool_name, "log"); assert_eq!(meta.call_id, "real-call-7"); assert!(meta.is_error);
        self.0 += 1; Ok(Transform::Unchanged)
    }
}
#[test]
fn invalid_custom_output_stops_the_chain_without_exposing_raw_output() {
    for invalid_utf8 in [false, true] {
        let raw = original("sensitive raw output"); let mut scratch = [0; 64]; let mut out = [0; 64];
        let mut pipeline = Chain::new(Bad { calls: 0, invalid_utf8 }, Counter(0), &mut scratch);
        assert!(matches!(raw.transform("log", &mut pipeline, &mut out), Err(Error::InvalidOutput)));
        assert_eq!(pipeline.first.calls, 1); assert_eq!(pipeline.second.0, 0);
        assert_eq!(raw.text, "sensitive raw output");
        assert!(matches!(raw.transform("log", &mut Bad { calls: 0, invalid_utf8 }, &mut out), Err(Error::InvalidOutput)));
    }
}
#[test]
fn chains_preserve_unchanged_and_rewritten_stages_and_support_nesting() {
    let mut scratch = [0; 128]; let mut second_scratch = [0; 128]; let mut out = [0; 128];
    let mut pipeline = Chain::new(Identity, Identity, &mut []);
    assert_eq!(original("same").transform("log", &mut pipeline, &mut []).unwrap().text, "same");
    let mut pipeline = Chain::new(Trim, Identity, &mut scratch);
    assert_eq!(original(" x ").transform("log", &mut pipeline, &mut out).unwrap().text, "x");
    assert!(matches!(original(" x ").transform("log", &mut pipeline, &mut []), Err(Error::Capacity { needed: 1 })));
    let mut pipeline = Chain::new(Chain::new(DedupAdjacentLines, Trim, &mut scratch), Truncate::new(9, "...").unwrap(), &mut second_scratch);
    assert_eq!(original("a\na\n0123456789\n").transform("log", &mut pipeline, &mut out).unwrap().text, "a\n0123...");
}
#[test]
fn hooks_pause_deny_and_order_are_distinct_from_lifecycle_commits() {
    use std::cell::RefCell;
    struct Trace<'a>(&'a RefCell<Vec<u8>>, u8, Decision);
    impl Hooks for Trace<'_> {
        fn before_result(&mut self, event: BeforeResult<'_>) -> Decision {
            assert!(event.meta.is_error); assert_eq!(event.text, "raw"); self.0.borrow_mut().push(self.1); self.2
        }
        fn after_result(&mut self, event: AfterResult<'_>) {
            assert_eq!(event.original_bytes, 3); assert_eq!(event.text, "raw"); self.0.borrow_mut().push(self.1 + 10);
        }
    }
    let trace = RefCell::new(Vec::new());
    let mut hooks = (Trace(&trace, 1, Decision::Pause), Trace(&trace, 2, Decision::Allow));
    let mut transform = Counter(0); let mut out = [0xaa; 8];
    assert!(matches!(original("raw").transform_with_hooks("log", &mut transform, &mut out, &mut hooks).unwrap(), HookResult::Paused));
    assert_eq!(transform.0, 0); assert_eq!(out, [0xaa; 8]); assert_eq!(*trace.borrow(), [1]);
    hooks.0.2 = Decision::Deny;
    assert!(matches!(original("raw").transform_with_hooks("log", &mut transform, &mut out, &mut hooks), Err(Error::Denied)));
    assert_eq!(transform.0, 0); assert_eq!(*trace.borrow(), [1, 1]);
    hooks.0.2 = Decision::Allow;
    assert!(matches!(original("raw").transform_with_hooks("log", &mut transform, &mut out, &mut &mut hooks).unwrap(), HookResult::Applied(_)));
    assert_eq!(transform.0, 1); assert_eq!(*trace.borrow(), [1, 1, 1, 2, 11, 12]);
    assert!(matches!(original("raw").transform_with_hooks("log", &mut Identity, &mut [], &mut NoHooks).unwrap(), HookResult::Applied(_)));
}
#[test]
fn after_result_does_not_fire_for_transform_failure() {
    struct Audit(usize);
    impl Hooks for Audit { fn after_result(&mut self, _: AfterResult<'_>) { self.0 += 1; } }
    let mut audit = Audit(0);
    assert!(original("x").transform_with_hooks("log", &mut Bad { calls: 0, invalid_utf8: false }, &mut [], &mut audit).is_err());
    assert_eq!(audit.0, 0);
}
