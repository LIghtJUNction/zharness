use super::{SkillError, valid_xml};
use core::fmt;

/// Frontmatter views borrowing the original `SKILL.md` source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SkillMetadata<'a> {
    name: &'a str,
    description: Description<'a>,
    body_offset: usize,
}
impl<'a> SkillMetadata<'a> {
    /// Parse only the frontmatter, accepting UTF-8 BOM and LF or CRLF lines.
    ///
    /// Names contain 1..=64 lowercase ASCII letters, digits or single hyphens,
    /// without a leading/trailing hyphen. Descriptions contain 1..=1024 Unicode
    /// scalar values after whitespace normalization. Unknown top-level keys and
    /// their indented values are skipped; nested `name`/`description` keys do not
    /// become skill metadata. Aliases, tags and flow collections for the required
    /// fields are unsupported. Quoted names must not require escape decoding.
    pub fn parse(source: &'a str) -> Result<Self, SkillError> {
        let start = source.strip_prefix('\u{feff}').map_or(0, |_| 3);
        let mut lines = Lines::new(&source[start..]);
        let first = lines.next().ok_or(SkillError::MissingFrontmatter)?;
        if !delimiter(first.text) {
            return Err(SkillError::MissingFrontmatter);
        }
        let header_start = lines.position;
        let (header_end, body_offset) = loop {
            let line = lines.next().ok_or(SkillError::UnterminatedFrontmatter)?;
            if delimiter(line.text) {
                break (line.start, start + lines.position);
            }
        };
        let header = &source[start + header_start..start + header_end];
        let mut lines = Lines::new(header);
        let mut name = None;
        let mut description = None;
        while let Some(line) = lines.next() {
            if line.text.trim().is_empty() || line.text.trim_start().starts_with('#') {
                continue;
            }
            if indented(line.text) {
                return Err(SkillError::InvalidFrontmatter);
            }
            let colon = line.text.find(':').ok_or(SkillError::InvalidFrontmatter)?;
            let key = line.text[..colon].trim();
            if key.is_empty() {
                return Err(SkillError::InvalidFrontmatter);
            }
            let value = &line.text[colon + 1..];
            let leading = value.len() - value.trim_start().len();
            let value_start = line.start + colon + 1 + leading;
            match key {
                "name" => {
                    if name.is_some() {
                        return Err(SkillError::DuplicateField);
                    }
                    let scalar = scalar(&mut lines, value_start, line.end)?;
                    let value = match scalar.style {
                        Style::Plain => plain_name(scalar.raw),
                        Style::Single | Style::Double => scalar.raw,
                        Style::Block => return Err(SkillError::InvalidName),
                    };
                    validate_name(value)?;
                    name = Some(value);
                }
                "description" => {
                    if description.is_some() {
                        return Err(SkillError::DuplicateField);
                    }
                    description = Some(scalar(&mut lines, value_start, line.end)?.validate()?);
                }
                _ => {
                    continuation_end(&mut lines, line.end);
                }
            }
        }
        Ok(Self {
            name: name.ok_or(SkillError::MissingName)?,
            description: description.ok_or(SkillError::MissingDescription)?,
            body_offset,
        })
    }

    pub const fn name(self) -> &'a str {
        self.name
    }

    pub const fn description(self) -> Description<'a> {
        self.description
    }

    /// Byte offset immediately after the closing frontmatter delimiter line.
    pub const fn body_offset(self) -> usize {
        self.body_offset
    }
}

/// A validated description which decodes on demand without an allocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Description<'a> {
    raw: &'a str,
    style: Style,
    len: usize,
}
impl Description<'_> {
    /// Number of UTF-8 bytes in the decoded, whitespace-normalized description.
    pub const fn len(self) -> usize {
        self.len
    }

    pub const fn is_empty(self) -> bool {
        self.len == 0
    }

    /// Decode and collapse whitespace; preserve `output` on capacity failure.
    pub fn write_to(self, output: &mut [u8]) -> Result<&str, SkillError> {
        if output.len() < self.len {
            return Err(SkillError::Capacity { needed: self.len });
        }
        let mut offset = 0;
        for character in self.chars() {
            let character = character?;
            let end = offset + character.len_utf8();
            character.encode_utf8(&mut output[offset..end]);
            offset = end;
        }
        core::str::from_utf8(&output[..self.len]).map_err(|_| SkillError::InvalidDescription)
    }

    pub(super) fn chars(self) -> impl Iterator<Item = Result<char, SkillError>> {
        Normalized {
            decoded: Decoded {
                raw: self.raw,
                style: self.style,
                position: 0,
                previous_space: true,
            },
            started: false,
            pending_space: false,
            buffered: None,
        }
    }

    fn validate(mut self) -> Result<Self, SkillError> {
        let mut count = 0;
        let mut len = 0;
        for character in self.chars() {
            let character = character?;
            if !valid_xml(character) {
                return Err(SkillError::InvalidDescription);
            }
            count += 1;
            if count > 1024 {
                return Err(SkillError::InvalidDescription);
            }
            len += character.len_utf8();
        }
        if count == 0 {
            return Err(SkillError::InvalidDescription);
        }
        self.len = len;
        Ok(self)
    }
}
impl fmt::Display for Description<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use fmt::Write;
        for character in self.chars() {
            f.write_char(character.map_err(|_| fmt::Error)?)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Style {
    Plain,
    Single,
    Double,
    Block,
}

fn delimiter(line: &str) -> bool {
    line.trim_end_matches([' ', '\t']) == "---"
}

fn validate_name(name: &str) -> Result<(), SkillError> {
    if name.is_empty()
        || name.len() > 64
        || name.starts_with('-')
        || name.ends_with('-')
        || name.contains("--")
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(SkillError::InvalidName);
    }
    Ok(())
}

fn plain_name(raw: &str) -> &str {
    let mut previous_space = true;
    for (index, character) in raw.char_indices() {
        if character == '#' && previous_space {
            return raw[..index].trim();
        }
        previous_space = character.is_whitespace();
    }
    raw.trim()
}

fn indented(line: &str) -> bool {
    line.starts_with([' ', '\t'])
}

struct Line<'a> {
    text: &'a str,
    start: usize,
    end: usize,
}
#[derive(Clone, Copy)]
struct Lines<'a> {
    source: &'a str,
    position: usize,
}
impl<'a> Lines<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            position: 0,
        }
    }
    fn next(&mut self) -> Option<Line<'a>> {
        if self.position == self.source.len() {
            return None;
        }
        let start = self.position;
        let remaining = &self.source[start..];
        let end = remaining
            .find('\n')
            .map_or(self.source.len(), |offset| start + offset);
        self.position = (end + usize::from(end < self.source.len())).min(self.source.len());
        let end = end - usize::from(self.source[start..end].ends_with('\r'));
        Some(Line {
            text: &self.source[start..end],
            start,
            end,
        })
    }
}

fn continuation_end(lines: &mut Lines<'_>, mut end: usize) -> usize {
    loop {
        let mut peek = *lines;
        let Some(line) = peek.next() else {
            break;
        };
        if line.text.trim().is_empty() || indented(line.text) {
            *lines = peek;
            end = line.end;
        } else {
            break;
        }
    }
    end
}

fn scalar<'a>(
    lines: &mut Lines<'a>,
    start: usize,
    line_end: usize,
) -> Result<Description<'a>, SkillError> {
    let value = &lines.source[start..line_end];
    let first = value.chars().next();
    let (raw, style) = match first {
        Some(quote @ ('\'' | '"')) => {
            let tail = &lines.source[start + 1..];
            let mut positions = tail.char_indices().peekable();
            let closing = loop {
                let Some((index, character)) = positions.next() else {
                    return Err(SkillError::InvalidFrontmatter);
                };
                if quote == '"' && character == '\\' {
                    positions.next().ok_or(SkillError::InvalidEscape)?;
                } else if character == quote {
                    if quote == '\'' && positions.peek().is_some_and(|(_, next)| *next == '\'') {
                        positions.next();
                    } else {
                        break start + 1 + index;
                    }
                }
            };
            let suffix_start = closing + 1;
            let suffix_end = lines.source[suffix_start..]
                .find('\n')
                .map_or(lines.source.len(), |offset| suffix_start + offset);
            let suffix = lines.source[suffix_start..suffix_end].trim();
            if !suffix.is_empty() && !suffix.starts_with('#') {
                return Err(SkillError::InvalidFrontmatter);
            }
            lines.position = suffix_end + usize::from(suffix_end < lines.source.len());
            (
                &lines.source[start + 1..closing],
                if quote == '\'' {
                    Style::Single
                } else {
                    Style::Double
                },
            )
        }
        Some('|' | '>') => {
            let marker = plain_name(value);
            let mut digit = false;
            let mut chomp = false;
            for byte in marker.as_bytes().iter().skip(1) {
                match byte {
                    b'1'..=b'9' if !digit => digit = true,
                    b'+' | b'-' if !chomp => chomp = true,
                    _ => return Err(SkillError::UnsupportedScalar),
                }
            }
            let content_start = lines.position;
            let end = continuation_end(lines, content_start);
            (&lines.source[content_start..end], Style::Block)
        }
        None | Some('[' | '{' | '&' | '*' | '!' | '@' | '`' | '%' | '#') => {
            return Err(SkillError::UnsupportedScalar);
        }
        _ => {
            let end = continuation_end(lines, line_end);
            (&lines.source[start..end], Style::Plain)
        }
    };
    Ok(Description { raw, style, len: 0 })
}

struct Decoded<'a> {
    raw: &'a str,
    style: Style,
    position: usize,
    previous_space: bool,
}
impl Decoded<'_> {
    fn take(&mut self) -> Option<char> {
        let character = self.raw[self.position..].chars().next()?;
        self.position += character.len_utf8();
        Some(character)
    }

    fn escape(&mut self) -> Result<Option<char>, SkillError> {
        let character = self.take().ok_or(SkillError::InvalidEscape)?;
        let escaped = match character {
            '"' | '/' | '\\' | ' ' => character,
            't' | '\t' => '\t',
            'n' => '\n',
            'r' => '\r',
            '0' => '\0',
            'a' => '\u{7}',
            'b' => '\u{8}',
            'v' => '\u{b}',
            'f' => '\u{c}',
            'e' => '\u{1b}',
            'N' => '\u{85}',
            '_' => '\u{a0}',
            'L' => '\u{2028}',
            'P' => '\u{2029}',
            'x' | 'u' | 'U' => {
                let width = match character {
                    'x' => 2,
                    'u' => 4,
                    _ => 8,
                };
                let mut value = 0u32;
                for _ in 0..width {
                    let digit = self
                        .take()
                        .and_then(|character| character.to_digit(16))
                        .ok_or(SkillError::InvalidEscape)?;
                    value = (value << 4) | digit;
                }
                char::from_u32(value).ok_or(SkillError::InvalidEscape)?
            }
            '\n' | '\r' => {
                while self.raw[self.position..]
                    .chars()
                    .next()
                    .is_some_and(char::is_whitespace)
                {
                    self.take();
                }
                return Ok(None);
            }
            _ => return Err(SkillError::InvalidEscape),
        };
        Ok(Some(escaped))
    }
}
impl Iterator for Decoded<'_> {
    type Item = Result<char, SkillError>;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let character = self.take()?;
            let decoded = match (self.style, character) {
                (Style::Plain, '#') if self.previous_space => {
                    while let Some(next) = self.take() {
                        if next == '\n' {
                            break;
                        }
                    }
                    '\n'
                }
                (Style::Single, '\'') => {
                    if self.take() != Some('\'') {
                        return Some(Err(SkillError::InvalidEscape));
                    }
                    '\''
                }
                (Style::Double, '\\') => match self.escape() {
                    Ok(Some(character)) => character,
                    Ok(None) => continue,
                    Err(error) => return Some(Err(error)),
                },
                _ => character,
            };
            self.previous_space = decoded.is_whitespace();
            return Some(Ok(decoded));
        }
    }
}

struct Normalized<'a> {
    decoded: Decoded<'a>,
    started: bool,
    pending_space: bool,
    buffered: Option<char>,
}
impl Iterator for Normalized<'_> {
    type Item = Result<char, SkillError>;
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(character) = self.buffered.take() {
            return Some(Ok(character));
        }
        for character in self.decoded.by_ref() {
            let character = match character {
                Ok(character) => character,
                Err(error) => return Some(Err(error)),
            };
            if character.is_whitespace() {
                self.pending_space = self.started;
                continue;
            }
            self.started = true;
            if self.pending_space {
                self.pending_space = false;
                self.buffered = Some(character);
                return Some(Ok(' '));
            }
            return Some(Ok(character));
        }
        None
    }
}
