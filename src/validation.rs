//! Tool argument validation delegates JSON grammar to zcall's public parser.
//! The additional pass applies string-size and integer-range resource policy.
use crate::{Error, zcall::v1::{Json, JsonKind, Limits, TokenStorage}};

#[derive(Clone, Copy, Debug)]
pub struct ArgumentLimits {
    pub json: Limits,
    pub max_string_bytes: usize,
    /// Reject integer lexemes outside i64 (negative) / u64 (nonnegative).
    /// False preserves arbitrary integer lexemes without converting or rounding.
    pub bounded_integers: bool,
}
impl Default for ArgumentLimits {
    fn default() -> Self {
        Self { json: Limits { max_depth: 32, max_input: 1 << 20, max_tokens: 4096, max_object_fields: 256 }, max_string_bytes: 65536, bounded_integers: true }
    }
}

/// A complete JSON object validated with explicit caller-selected limits.
#[derive(Clone, Copy, Debug)]
pub struct Arguments<'a> { json: Json<'a> }
impl<'a> Arguments<'a> {
    pub fn parse<const N: usize>(input: &'a [u8], tokens: &mut TokenStorage<N>, limits: ArgumentLimits) -> Result<Self, Error> {
        let json = Json::parse(input, tokens, limits.json)?;
        if json.kind() != JsonKind::Object { return Err(Error::InvalidArguments { offset: 0 }); }
        check_scalars(input, limits)?;
        Ok(Self { json })
    }
    pub fn as_bytes(self) -> &'a [u8] { self.json.raw() }
    pub fn json(self) -> Json<'a> { self.json }
}

/// Application-defined schema/business checks. This is not a JSON Schema engine.
pub trait ValidationPolicy {
    fn validate(&mut self, name: &str, arguments: Arguments<'_>) -> Result<(), Error>;
}

fn check_scalars(input: &[u8], limits: ArgumentLimits) -> Result<(), Error> {
    // Grammar, UTF-8 and escape validity have already been checked by Json::parse.
    // This walk never decides whether syntax is valid and never constructs an IR.
    let mut at = 0;
    while at < input.len() {
        if input[at] == b'"' {
            let start = at;
            at += 1;
            let mut escaped = false;
            while input[at] != b'"' {
                if input[at] == b'\\' { escaped = true; at += 2; } else { at += 1; }
            }
            at += 1;
            let size = if escaped {
                let mut scalar_tokens = TokenStorage::<1>::new();
                let value = Json::parse(&input[start..at], &mut scalar_tokens, limits.json)?;
                value.as_text().ok_or(Error::InvalidArguments { offset: start })?.decoded_len()
            } else { at - start - 2 };
            if size > limits.max_string_bytes { return Err(Error::StringLimit { offset: start, limit: limits.max_string_bytes }); }
        } else if input[at] == b'-' || input[at].is_ascii_digit() {
            let start = at;
            while at < input.len() && matches!(input[at], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') { at += 1; }
            let raw = &input[start..at];
            if limits.bounded_integers && !raw.iter().any(|b| matches!(b, b'.' | b'e' | b'E')) {
                let text = core::str::from_utf8(raw).map_err(|_| Error::InvalidArguments { offset: start })?;
                let overflow = if raw[0] == b'-' { text.parse::<i64>().is_err() } else { text.parse::<u64>().is_err() };
                if overflow { return Err(Error::IntegerOverflow { offset: start }); }
            }
        } else { at += 1; }
    }
    Ok(())
}
