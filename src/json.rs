//! Small bounded JSON structure scanner. It validates structure without allocation.

use crate::mcp::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Kind { Object, Array, String, Scalar }
#[derive(Clone, Copy, Debug)]
pub(crate) struct Value { pub start: usize, pub end: usize, pub kind: Kind }
impl Value { pub fn bytes<'a>(self, input: &'a [u8]) -> &'a [u8] { &input[self.start..self.end] } }

fn ws(b: u8) -> bool { matches!(b, b' ' | b'\n' | b'\r' | b'\t') }
fn string_end(input: &[u8], start: usize) -> Result<usize, Error> {
    if input.get(start) != Some(&b'"') { return Err(Error::InvalidJson); }
    let mut i = start + 1;
    while let Some(&b) = input.get(i) {
        match b {
            b'"' => return Ok(i + 1),
            b'\\' => {
                i += 1;
                match input.get(i).copied() {
                    Some(b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') => i += 1,
                    Some(b'u') => {
                        for _ in 0..4 { i += 1; if !input.get(i).is_some_and(u8::is_ascii_hexdigit) { return Err(Error::InvalidJson); } }
                        i += 1;
                    }
                    _ => return Err(Error::InvalidJson),
                }
            }
            0..=0x1f => return Err(Error::InvalidJson),
            _ => i += 1,
        }
    }
    Err(Error::InvalidJson)
}
fn skip_ws(input: &[u8], mut i: usize) -> usize { while input.get(i).is_some_and(|b| ws(*b)) { i += 1; } i }
fn value_end(input: &[u8], start: usize) -> Result<Value, Error> {
    let Some(&first) = input.get(start) else { return Err(Error::InvalidJson); };
    if first == b'"' { return Ok(Value { start, end: string_end(input, start)?, kind: Kind::String }); }
    if first != b'{' && first != b'[' {
        let mut i = start;
        while let Some(&b) = input.get(i) { if ws(b) || b == b',' || b == b']' || b == b'}' { break; } i += 1; }
        if i == start { return Err(Error::InvalidJson); }
        return Ok(Value { start, end: i, kind: Kind::Scalar });
    }
    let mut stack = [0u8; 128];
    let mut depth = 0usize;
    let mut i = start;
    while let Some(&b) = input.get(i) {
        match b {
            b'"' => i = string_end(input, i)?,
            b'{' | b'[' => { if depth == stack.len() { return Err(Error::DepthLimit); } stack[depth] = if b == b'{' { b'}' } else { b']' }; depth += 1; i += 1; }
            b'}' | b']' => { if depth == 0 || stack[depth - 1] != b { return Err(Error::InvalidJson); } depth -= 1; i += 1; if depth == 0 { return Ok(Value { start, end: i, kind: if first == b'{' { Kind::Object } else { Kind::Array } }); } }
            _ => i += 1,
        }
    }
    Err(Error::InvalidJson)
}

pub(crate) fn root(input: &[u8]) -> Result<Value, Error> {
    let start = skip_ws(input, 0);
    let value = value_end(input, start)?;
    if skip_ws(input, value.end) != input.len() { return Err(Error::InvalidJson); }
    if value.kind != Kind::Object { return Err(Error::InvalidJson); }
    Ok(value)
}

/// Returns an unescaped object key and its value. Escaped keys are rejected.
pub(crate) fn field<'a>(input: &'a [u8], object: Value, key: &[u8]) -> Result<Option<Value>, Error> {
    if object.kind != Kind::Object { return Err(Error::InvalidJson); }
    let mut i = skip_ws(input, object.start + 1);
    if input.get(i) == Some(&b'}') { return Ok(None); }
    loop {
        let end = string_end(input, i)?;
        let raw = &input[i + 1..end - 1];
        if raw.contains(&b'\\') { return Err(Error::InvalidJson); }
        i = skip_ws(input, end);
        if input.get(i) != Some(&b':') { return Err(Error::InvalidJson); }
        i = skip_ws(input, i + 1);
        let value = value_end(input, i)?;
        if raw == key { return Ok(Some(value)); }
        i = skip_ws(input, value.end);
        match input.get(i) { Some(b',') => i = skip_ws(input, i + 1), Some(b'}') => return Ok(None), _ => return Err(Error::InvalidJson) }
    }
}

pub(crate) fn string<'a>(input: &'a [u8], value: Value) -> Result<&'a [u8], Error> {
    if value.kind != Kind::String || input.get(value.start) != Some(&b'"') { return Err(Error::InvalidJson); }
    let bytes = &input[value.start + 1..value.end - 1];
    if bytes.contains(&b'\\') { return Err(Error::InvalidJson); }
    core::str::from_utf8(bytes).map_err(|_| Error::InvalidJson)?;
    Ok(bytes)
}
