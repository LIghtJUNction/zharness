//! Local result-processing example: no network/model request is performed.
use zharness::{
    Error, Executor, Invocation, Tool, ToolOutput, ToolResultTransform, ToolSet,
    context::{Budget, Context, Group, Message, Overflow},
    hooks::{AfterResult, BeforeResult, Decision, HookResult, Hooks},
    output::{Chain, DedupAdjacentLines, JsonMinify, ResultMeta, Transform},
    validation::{ArgumentLimits, Arguments},
    zcall::v1::*,
};

struct Build {
    calls: usize,
}
impl Executor for Build {
    fn execute(
        &mut self,
        name: &str,
        _: Arguments<'_>,
        output: &mut [u8],
    ) -> Result<ToolOutput, Error> {
        if name != "build" {
            return Err(Error::UnknownTool);
        }
        let log = b"DEBUG: scanning files\nDEBUG: scanning files\nerror: missing config\nerror: missing config\n";
        output
            .get_mut(..log.len())
            .ok_or(Error::Capacity { needed: log.len() })?
            .copy_from_slice(log);
        self.calls += 1;
        Ok(ToolOutput::failure(log.len()))
    }
}
/// Application-specific policy: discard only DEBUG-prefixed lines in this log format.
struct DropDebug;
impl ToolResultTransform for DropDebug {
    fn transform(
        &mut self,
        _: ResultMeta<'_>,
        input: &str,
        output: &mut [u8],
    ) -> Result<Transform, Error> {
        let lines = || {
            input
                .split_inclusive('\n')
                .filter(|line| !line.starts_with("DEBUG:"))
        };
        let needed: usize = lines().map(str::len).sum();
        if needed == input.len() {
            return Ok(Transform::Unchanged);
        }
        let dst = output.get_mut(..needed).ok_or(Error::Capacity { needed })?;
        let mut at = 0;
        for line in lines() {
            dst[at..at + line.len()].copy_from_slice(line.as_bytes());
            at += line.len();
        }
        Ok(Transform::Written(at))
    }
}
#[derive(Default)]
struct Audit {
    processed: usize,
    saved_bytes: usize,
}
impl Hooks for Audit {
    fn before_result(&mut self, event: BeforeResult<'_>) -> Decision {
        if event.meta.tool_name == "build" {
            Decision::Allow
        } else {
            Decision::Deny
        }
    }
    fn after_result(&mut self, event: AfterResult<'_>) {
        self.processed += 1;
        self.saved_bytes += event.original_bytes.saturating_sub(event.text.len());
    }
}

pub fn main() -> Result<(), Error> {
    let definitions = [Tool::new(
        "build",
        "Build with application authorization",
        r#"{"type":"object"}"#,
    )];
    let registry = ToolSet::new(&definitions)?;
    // A local demonstration ID. Real model flows must retain the ReadyTool ID.
    let call = Invocation {
        signature: None,
        call_id: "demo-build-1",
        name: "build",
        arguments: b"{}",
    };
    let mut tokens = TokenStorage::<32>::new();
    let prepared = call.prepare(&registry, &mut tokens, ArgumentLimits::default())?;
    let mut raw_storage = [0; 256];
    let mut scratch = [0; 256];
    let mut reduced_storage = [0; 256];
    let mut executor = Build { calls: 0 };
    // The application explicitly authorizes this harmless, simulated build.
    let original = prepared.execute(&mut executor, &mut raw_storage)?;
    let mut pipeline = Chain::new(DropDebug, DedupAdjacentLines, &mut scratch);
    let mut audit = Audit::default();
    let HookResult::Applied(reduced) =
        original.transform_with_hooks("build", &mut pipeline, &mut reduced_storage, &mut audit)?
    else {
        return Err(Error::InvalidState);
    };
    assert_eq!(executor.calls, 1);
    assert_eq!(reduced.call_id, original.call_id);
    assert!(reduced.is_error); // Compact text does not turn a failed tool into success.
    assert_eq!(reduced.text, "error: missing config\n");
    assert!(original.text.contains("DEBUG:")); // Retained raw result is untouched.

    let calls = [prepared.content()];
    let messages = [
        Message::new(Role::Assistant, &calls),
        Message::ToolResult(reduced),
    ];
    let mut slots = [None; 1];
    let mut context = Context::new(
        &mut slots,
        Budget {
            limit: 4096,
            reserved: 1024,
        },
    )?;
    // Illustrative cost only; production callers count tokens after reduction.
    context.push(Group::new(1, &messages, 64), Overflow::Reject)?;
    let mut records = RecordStorage::<16>::new();
    let request = context.build(
        Protocol::AnthropicMessages.into(),
        Presence::Value(Text::new("model-x")),
        &mut records,
        &[],
    )?;
    assert_eq!(request.messages().count(), 2);

    let raw_json = zharness::bridge::Completion {
        call_id: "demo-json-2",
        text: r#" { "ok" : true, "text" : "keep  spaces" } "#,
        is_error: false,
    };
    let mut json_storage = [0; 128];
    let json = raw_json.transform(
        "query",
        &mut JsonMinify::new(&mut tokens, Limits::default()),
        &mut json_storage,
    )?;
    assert_eq!(json.text, r#"{"ok":true,"text":"keep  spaces"}"#);
    println!(
        "Saved {} bytes; original error status retained.\n{}",
        audit.saved_bytes, reduced.text
    );
    println!("Validated JSON: {}", json.text);
    Ok(())
}
