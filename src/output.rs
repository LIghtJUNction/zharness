//! Caller-buffered tool-result transforms, before history/session commitment.
//!
//! Transforms receive immutable identity/status metadata and cannot replace it.
//! Original output remains available on pause, denial or failure. Retry only this
//! stage with retained input, never a side-effecting tool. No model is called here.
use crate::{
    Error,
    bridge::Completion,
    hooks::{AfterResult, BeforeResult, Decision, HookResult, Hooks},
    zcall::v1::{Json, Limits, TokenStorage},
};

/// Identity/status supplied to a transform or result hook, never returned by it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResultMeta<'a> {
    pub tool_name: &'a str,
    pub call_id: &'a str,
    pub is_error: bool,
}

/// An unchanged result borrows the original text and needs no output capacity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub enum Transform {
    Unchanged,
    /// Number of UTF-8 bytes written to the supplied output slice.
    Written(usize),
}

/// A synchronous output-processing hook. Implement schema-aware extraction,
/// redaction or application summaries without a mandatory runtime or allocation.
///
/// Partial output/scratch must be discarded on error. Implementations may be
/// retried with the same retained input and must not re-execute the original tool.
/// Output bytes are checked before creating a Completion or invoking the next
/// Chain stage. JSON/schema/semantic preservation is a separate application policy.
pub trait ToolResultTransform {
    fn transform(
        &mut self,
        meta: ResultMeta<'_>,
        input: &str,
        output: &mut [u8],
    ) -> Result<Transform, Error>;
}
impl<T: ToolResultTransform + ?Sized> ToolResultTransform for &mut T {
    fn transform(&mut self, meta: ResultMeta<'_>, input: &str, output: &mut [u8]) -> Result<Transform, Error> {
        (**self).transform(meta, input, output)
    }
}

fn resolve<'a>(result: Transform, input: &'a str, output: &'a [u8]) -> Result<&'a str, Error> {
    match result {
        Transform::Unchanged => Ok(input),
        Transform::Written(n) => core::str::from_utf8(output.get(..n).ok_or(Error::InvalidOutput)?)
            .map_err(|_| Error::InvalidOutput),
    }
}
fn write_text(text: &str, output: &mut [u8]) -> Result<Transform, Error> {
    output.get_mut(..text.len()).ok_or(Error::Capacity { needed: text.len() })?
        .copy_from_slice(text.as_bytes());
    Ok(Transform::Written(text.len()))
}

impl<'c, 'o> Completion<'c, 'o> {
    /// Transform retained text, preserving the exact call ID and error flag.
    /// The returned text borrows either the original input or caller output.
    pub fn transform<'b, T: ToolResultTransform + ?Sized>(
        self,
        tool_name: &str,
        transform: &mut T,
        output: &'b mut [u8],
    ) -> Result<Completion<'c, 'b>, Error>
    where
        'o: 'b,
    {
        let meta = ResultMeta { tool_name, call_id: self.call_id, is_error: self.is_error };
        let result = transform.transform(meta, self.text, output)?;
        let text = resolve(result, self.text, output)?;
        Ok(Completion { call_id: self.call_id, text, is_error: self.is_error })
    }

    /// Result-specific gates run before processing; observers run only after a
    /// valid result exists. This does not append history or complete a ToolLoop
    /// ticket. Existing before/after lifecycle callbacks keep their old timing.
    pub fn transform_with_hooks<'b, T: ToolResultTransform + ?Sized>(
        self,
        tool_name: &str,
        transform: &mut T,
        output: &'b mut [u8],
        hooks: &mut impl Hooks,
    ) -> Result<HookResult<Completion<'c, 'b>>, Error>
    where
        'o: 'b,
    {
        let meta = ResultMeta { tool_name, call_id: self.call_id, is_error: self.is_error };
        match hooks.before_result(BeforeResult { meta, text: self.text }) {
            Decision::Pause => return Ok(HookResult::Paused),
            Decision::Deny => return Err(Error::Denied),
            Decision::Allow => {}
        }
        let result = self.transform(tool_name, transform, output)?;
        hooks.after_result(AfterResult { meta, original_bytes: self.text.len(), text: result.text });
        Ok(HookResult::Applied(result))
    }
}

/// No-op, zero-copy processing, including when output is empty.
#[derive(Clone, Copy, Debug, Default)]
pub struct Identity;
impl ToolResultTransform for Identity {
    fn transform(&mut self, _: ResultMeta<'_>, _: &str, _: &mut [u8]) -> Result<Transform, Error> {
        Ok(Transform::Unchanged)
    }
}

/// Trim leading/trailing Unicode whitespace from plain text. Explicitly lossy.
#[derive(Clone, Copy, Debug, Default)]
pub struct Trim;
impl ToolResultTransform for Trim {
    fn transform(&mut self, _: ResultMeta<'_>, input: &str, output: &mut [u8]) -> Result<Transform, Error> {
        let trimmed = input.trim();
        if trimmed.len() == input.len() { Ok(Transform::Unchanged) } else { write_text(trimmed, output) }
    }
}

/// Remove adjacent byte-identical lines, including their line terminators.
/// Nonadjacent repetitions and differing CR/LF endings are retained. Lossy: a
/// repeated line may be significant; apply only to logs whose policy permits it.
#[derive(Clone, Copy, Debug, Default)]
pub struct DedupAdjacentLines;
impl ToolResultTransform for DedupAdjacentLines {
    fn transform(&mut self, _: ResultMeta<'_>, input: &str, output: &mut [u8]) -> Result<Transform, Error> {
        let mut previous = None;
        let needed: usize = input.split_inclusive('\n').filter(|line| {
            let keep = previous != Some(*line);
            previous = Some(*line);
            keep
        }).map(str::len).sum();
        if needed == input.len() { return Ok(Transform::Unchanged); }
        let output = output.get_mut(..needed).ok_or(Error::Capacity { needed })?;
        previous = None;
        let mut at = 0;
        for line in input.split_inclusive('\n') {
            if previous != Some(line) {
                output[at..at + line.len()].copy_from_slice(line.as_bytes());
                at += line.len();
            }
            previous = Some(line);
        }
        Ok(Transform::Written(at))
    }
}

/// Explicit byte-budget truncation for plain text, never a JSON/token truncator.
/// UTF-8 code points are not split. The nonempty marker is included in the budget;
/// semantic facts and grapheme clusters may still be cut. No silent truncation.
#[derive(Clone, Copy, Debug)]
pub struct Truncate<'a> { max_bytes: usize, marker: &'a str }
impl<'a> Truncate<'a> {
    pub fn new(max_bytes: usize, marker: &'a str) -> Result<Self, Error> {
        if marker.is_empty() || marker.len() > max_bytes { return Err(Error::InvalidBudget); }
        Ok(Self { max_bytes, marker })
    }
}
impl ToolResultTransform for Truncate<'_> {
    fn transform(&mut self, _: ResultMeta<'_>, input: &str, output: &mut [u8]) -> Result<Transform, Error> {
        if input.len() <= self.max_bytes { return Ok(Transform::Unchanged); }
        let mut end = self.max_bytes - self.marker.len();
        while !input.is_char_boundary(end) { end -= 1; }
        let needed = end + self.marker.len();
        let dst = output.get_mut(..needed).ok_or(Error::Capacity { needed })?;
        dst[..end].copy_from_slice(&input.as_bytes()[..end]);
        dst[end..].copy_from_slice(self.marker.as_bytes());
        Ok(Transform::Written(needed))
    }
}

/// Validate using zcall's public parser, then remove JSON whitespace outside
/// strings. Keys, string escapes, number lexemes and array order are preserved.
/// This is not JSON canonicalization, field filtering or JSON Schema validation.
pub struct JsonMinify<'a, const N: usize> { tokens: &'a mut TokenStorage<N>, limits: Limits }
impl<'a, const N: usize> JsonMinify<'a, N> {
    pub fn new(tokens: &'a mut TokenStorage<N>, limits: Limits) -> Self { Self { tokens, limits } }
}
/// Syntax is already validated; this scan only selects insignificant whitespace.
fn json_bytes(input: &str, mut emit: impl FnMut(u8)) {
    let mut quoted = false;
    let mut escaped = false;
    for byte in input.bytes() {
        if quoted || !matches!(byte, b' ' | b'\t' | b'\n' | b'\r') { emit(byte); }
        if quoted {
            if escaped { escaped = false; }
            else if byte == b'\\' { escaped = true; }
            else if byte == b'"' { quoted = false; }
        } else if byte == b'"' { quoted = true; }
    }
}
impl<const N: usize> ToolResultTransform for JsonMinify<'_, N> {
    fn transform(&mut self, _: ResultMeta<'_>, input: &str, output: &mut [u8]) -> Result<Transform, Error> {
        Json::parse(input.as_bytes(), self.tokens, self.limits)?;
        let mut needed = 0;
        json_bytes(input, |_| needed += 1);
        if needed == input.len() { return Ok(Transform::Unchanged); }
        let dst = output.get_mut(..needed).ok_or(Error::Capacity { needed })?;
        let mut at = 0;
        json_bytes(input, |byte| { dst[at] = byte; at += 1; });
        Ok(Transform::Written(needed))
    }
}

/// Ordered transform composition with caller-owned intermediate storage.
/// Nest chains for more stages; each chain owns only a scratch borrow. A failure
/// stops later stages, never falls back to exposing unprocessed original output.
pub struct Chain<'s, A, B> { pub first: A, pub second: B, scratch: &'s mut [u8] }
impl<'s, A, B> Chain<'s, A, B> {
    pub fn new(first: A, second: B, scratch: &'s mut [u8]) -> Self { Self { first, second, scratch } }
    /// This chain descriptor and its scratch only, not nested buffers/payloads.
    pub fn memory_bytes(&self) -> usize { core::mem::size_of::<Self>() + self.scratch.len() }
}
impl<A: ToolResultTransform, B: ToolResultTransform> ToolResultTransform for Chain<'_, A, B> {
    fn transform(&mut self, meta: ResultMeta<'_>, input: &str, output: &mut [u8]) -> Result<Transform, Error> {
        let first = self.first.transform(meta, input, self.scratch)?;
        let intermediate = resolve(first, input, self.scratch)?;
        let second = self.second.transform(meta, intermediate, output)?;
        match second {
            Transform::Unchanged if first == Transform::Unchanged => Ok(Transform::Unchanged),
            Transform::Unchanged => write_text(intermediate, output),
            Transform::Written(n) => {
                resolve(second, intermediate, output)?;
                Ok(Transform::Written(n))
            }
        }
    }
}
