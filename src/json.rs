//! Bounded JSON scanner. It validates the complete JSON grammar without allocation.

use crate::mcp::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Kind {
    Object,
    Array,
    String,
    Scalar,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Value {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
}

impl Value {
    pub fn bytes(self, input: &[u8]) -> &[u8] {
        &input[self.start..self.end]
    }
}

const MAX_DEPTH: usize = 128;

fn ws(b: u8) -> bool {
    matches!(b, b' ' | b'\n' | b'\r' | b'\t')
}

struct Parser<'a> {
    input: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn skip_ws(&mut self) {
        while self.input.get(self.at).is_some_and(|b| ws(*b)) {
            self.at += 1;
        }
    }

    fn string(&mut self) -> Result<(), Error> {
        if self.input.get(self.at) != Some(&b'"') {
            return Err(Error::InvalidJson);
        }
        self.at += 1;
        let start = self.at;
        loop {
            match self.input.get(self.at).copied() {
                Some(b'"') => {
                    core::str::from_utf8(&self.input[start..self.at])
                        .map_err(|_| Error::InvalidJson)?;
                    self.at += 1;
                    return Ok(());
                }
                Some(b'\\') => {
                    self.at += 1;
                    match self.input.get(self.at).copied() {
                        Some(b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') => {
                            self.at += 1;
                        }
                        Some(b'u') => {
                            self.at += 1;
                            for _ in 0..4 {
                                if !self.input.get(self.at).is_some_and(u8::is_ascii_hexdigit) {
                                    return Err(Error::InvalidJson);
                                }
                                self.at += 1;
                            }
                        }
                        _ => return Err(Error::InvalidJson),
                    }
                }
                Some(0..=0x1f) | None => return Err(Error::InvalidJson),
                Some(_) => self.at += 1,
            }
        }
    }

    fn number(&mut self) -> Result<(), Error> {
        let start = self.at;
        if self.input.get(self.at) == Some(&b'-') {
            self.at += 1;
        }
        match self.input.get(self.at).copied() {
            Some(b'0') => {
                self.at += 1;
                if self.input.get(self.at).is_some_and(u8::is_ascii_digit) {
                    return Err(Error::InvalidJson);
                }
            }
            Some(b'1'..=b'9') => {
                self.at += 1;
                while self.input.get(self.at).is_some_and(u8::is_ascii_digit) {
                    self.at += 1;
                }
            }
            _ => return Err(Error::InvalidJson),
        }
        if self.input.get(self.at) == Some(&b'.') {
            self.at += 1;
            let before = self.at;
            while self.input.get(self.at).is_some_and(u8::is_ascii_digit) {
                self.at += 1;
            }
            if before == self.at {
                return Err(Error::InvalidJson);
            }
        }
        if matches!(self.input.get(self.at), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.input.get(self.at), Some(b'+' | b'-')) {
                self.at += 1;
            }
            let before = self.at;
            while self.input.get(self.at).is_some_and(u8::is_ascii_digit) {
                self.at += 1;
            }
            if before == self.at {
                return Err(Error::InvalidJson);
            }
        }
        if self.at == start {
            return Err(Error::InvalidJson);
        }
        Ok(())
    }

    fn value(&mut self, depth: usize) -> Result<Kind, Error> {
        self.skip_ws();
        if depth > MAX_DEPTH {
            return Err(Error::DepthLimit);
        }
        match self.input.get(self.at).copied() {
            Some(b'{') => {
                self.at += 1;
                self.skip_ws();
                if self.input.get(self.at) == Some(&b'}') {
                    self.at += 1;
                    return Ok(Kind::Object);
                }
                loop {
                    self.string()?;
                    self.skip_ws();
                    if self.input.get(self.at) != Some(&b':') {
                        return Err(Error::InvalidJson);
                    }
                    self.at += 1;
                    self.value(depth + 1)?;
                    self.skip_ws();
                    match self.input.get(self.at) {
                        Some(b',') => {
                            self.at += 1;
                            self.skip_ws();
                        }
                        Some(b'}') => {
                            self.at += 1;
                            return Ok(Kind::Object);
                        }
                        _ => return Err(Error::InvalidJson),
                    }
                }
            }
            Some(b'[') => {
                self.at += 1;
                self.skip_ws();
                if self.input.get(self.at) == Some(&b']') {
                    self.at += 1;
                    return Ok(Kind::Array);
                }
                loop {
                    self.value(depth + 1)?;
                    self.skip_ws();
                    match self.input.get(self.at) {
                        Some(b',') => {
                            self.at += 1;
                            self.skip_ws();
                        }
                        Some(b']') => {
                            self.at += 1;
                            return Ok(Kind::Array);
                        }
                        _ => return Err(Error::InvalidJson),
                    }
                }
            }
            Some(b'"') => {
                self.string()?;
                Ok(Kind::String)
            }
            Some(b't') if self.input.get(self.at..self.at + 4) == Some(b"true") => {
                self.at += 4;
                Ok(Kind::Scalar)
            }
            Some(b'f') if self.input.get(self.at..self.at + 5) == Some(b"false") => {
                self.at += 5;
                Ok(Kind::Scalar)
            }
            Some(b'n') if self.input.get(self.at..self.at + 4) == Some(b"null") => {
                self.at += 4;
                Ok(Kind::Scalar)
            }
            Some(b'-' | b'0'..=b'9') => {
                self.number()?;
                Ok(Kind::Scalar)
            }
            _ => Err(Error::InvalidJson),
        }
    }
}

pub(crate) fn root(input: &[u8]) -> Result<Value, Error> {
    let mut parser = Parser { input, at: 0 };
    parser.skip_ws();
    let start = parser.at;
    let kind = parser.value(0)?;
    let end = parser.at;
    parser.skip_ws();
    if parser.at != input.len() || kind != Kind::Object {
        return Err(Error::InvalidJson);
    }
    Ok(Value { start, end, kind })
}

/// Returns the value of an unescaped key. Duplicate occurrences are rejected.
pub(crate) fn field(
    input: &[u8],
    object: Value,
    key: &[u8],
) -> Result<Option<Value>, Error> {
    if object.kind != Kind::Object {
        return Err(Error::InvalidJson);
    }
    let mut parser = Parser {
        input,
        at: object.start + 1,
    };
    parser.skip_ws();
    if input.get(parser.at) == Some(&b'}') {
        return Ok(None);
    }
    let mut found = None;
    loop {
        let key_start = parser.at;
        parser.string()?;
        let key_end = parser.at;
        let raw = &input[key_start + 1..key_end - 1];
        parser.skip_ws();
        if input.get(parser.at) != Some(&b':') {
            return Err(Error::InvalidJson);
        }
        parser.at += 1;
        parser.skip_ws();
        let value_start = parser.at;
        let kind = parser.value(1)?;
        let value = Value {
            start: value_start,
            end: parser.at,
            kind,
        };
        if raw == key {
            if found.is_some() {
                return Err(Error::InvalidJson);
            }
            found = Some(value);
        }
        parser.skip_ws();
        match input.get(parser.at) {
            Some(b',') => {
                parser.at += 1;
                parser.skip_ws();
            }
            Some(b'}') => return Ok(found),
            _ => return Err(Error::InvalidJson),
        }
    }
}

/// Returns an unescaped UTF-8 string slice. Escaped strings remain available via Value::bytes.
pub(crate) fn string(input: &[u8], value: Value) -> Result<&[u8], Error> {
    if value.kind != Kind::String || input.get(value.start) != Some(&b'"') {
        return Err(Error::InvalidJson);
    }
    let bytes = &input[value.start + 1..value.end - 1];
    if bytes.contains(&b'\\') {
        return Err(Error::InvalidJson);
    }
    core::str::from_utf8(bytes).map_err(|_| Error::InvalidJson)?;
    Ok(bytes)
}
