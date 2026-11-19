//! Borrowed Agent Skills metadata and caller-buffered prompt summaries.
//!
//! Discovery needs only `name` and `description` from `SKILL.md` frontmatter.
//! Parsing stops at its closing delimiter; instructions and bundled resources
//! can be loaded separately when the application selects a skill.
//!
//! The parser implements the string forms used by skill metadata, not all YAML.
//! Plain, quoted and indented block descriptions are decoded and their whitespace
//! is collapsed to one space. Names use a portable ASCII subset of Agent Skills.

mod parse;

#[cfg(feature = "std")]
mod fs;
#[cfg(feature = "std")]
pub use fs::{
    DiscoveryLimit, DiscoveryLimits, LoadedSkill, SKILLS_PATH, SkillCatalog, SkillDiagnostic,
    SkillDiagnosticKind, SkillEntry, SkillLoadError, SkillPaths,
};

use core::fmt;
pub use parse::{Description, SkillMetadata};

/// A bounded parse or output failure. No partially written summary is returned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SkillError {
    MissingFrontmatter,
    UnterminatedFrontmatter,
    InvalidFrontmatter,
    MissingName,
    MissingDescription,
    DuplicateField,
    InvalidName,
    InvalidDescription,
    UnsupportedScalar,
    InvalidEscape,
    InvalidSummary,
    Capacity { needed: usize },
    Overflow,
}
impl fmt::Display for SkillError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl core::error::Error for SkillError {}

/// One already normalized metadata entry and its application-owned location.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SkillSummary<'a> {
    pub name: &'a str,
    pub description: &'a str,
    pub location: &'a str,
}

/// Return the exact UTF-8 capacity needed for an escaped XML skill catalog.
pub fn summaries_len<'a>(
    skills: impl IntoIterator<Item = SkillSummary<'a>>,
) -> Result<usize, SkillError> {
    render_summaries(skills.into_iter(), None)
}

/// Render metadata as `<available_skills>` XML, escaping every text field.
///
/// The iterator must be repeatable because validation and capacity are checked
/// before writing. On any input or capacity error, `output` remains unchanged.
/// The returned slice borrows only `output`.
pub fn write_summaries<'a, 'o, I>(skills: I, output: &'o mut [u8]) -> Result<&'o str, SkillError>
where
    I: IntoIterator<Item = SkillSummary<'a>>,
    I::IntoIter: Clone,
{
    let skills = skills.into_iter();
    let needed = render_summaries(skills.clone(), None)?;
    if output.len() < needed {
        return Err(SkillError::Capacity { needed });
    }
    render_summaries(skills, Some(&mut output[..needed]))?;
    core::str::from_utf8(&output[..needed]).map_err(|_| SkillError::InvalidSummary)
}

impl SkillMetadata<'_> {
    /// Exact capacity for this skill's escaped XML summary.
    pub fn summary_len(self, location: &str) -> Result<usize, SkillError> {
        render_metadata(self, location, None)
    }

    /// Format this skill without allocating an intermediate decoded description.
    pub fn write_summary<'o>(
        self,
        location: &str,
        output: &'o mut [u8],
    ) -> Result<&'o str, SkillError> {
        let needed = self.summary_len(location)?;
        if output.len() < needed {
            return Err(SkillError::Capacity { needed });
        }
        render_metadata(self, location, Some(&mut output[..needed]))?;
        core::str::from_utf8(&output[..needed]).map_err(|_| SkillError::InvalidSummary)
    }
}

fn render_summaries<'a>(
    skills: impl Iterator<Item = SkillSummary<'a>>,
    output: Option<&mut [u8]>,
) -> Result<usize, SkillError> {
    let mut writer = XmlWriter { output, len: 0 };
    writer.raw("<available_skills>\n")?;
    for skill in skills {
        writer.skill(
            skill.name,
            skill.description.chars().map(Ok),
            skill.location,
        )?;
    }
    writer.raw("</available_skills>\n")?;
    Ok(writer.len)
}

fn render_metadata(
    skill: SkillMetadata<'_>,
    location: &str,
    output: Option<&mut [u8]>,
) -> Result<usize, SkillError> {
    let mut writer = XmlWriter { output, len: 0 };
    writer.raw("<available_skills>\n")?;
    writer.skill(skill.name(), skill.description().chars(), location)?;
    writer.raw("</available_skills>\n")?;
    Ok(writer.len)
}

struct XmlWriter<'a> {
    output: Option<&'a mut [u8]>,
    len: usize,
}
impl XmlWriter<'_> {
    fn raw(&mut self, text: &str) -> Result<(), SkillError> {
        let end = self
            .len
            .checked_add(text.len())
            .ok_or(SkillError::Overflow)?;
        if let Some(output) = self.output.as_deref_mut() {
            output[self.len..end].copy_from_slice(text.as_bytes());
        }
        self.len = end;
        Ok(())
    }

    fn text(
        &mut self,
        chars: impl Iterator<Item = Result<char, SkillError>>,
    ) -> Result<(), SkillError> {
        for character in chars {
            let character = character?;
            let mut utf8 = [0; 4];
            let text = match character {
                '&' => "&amp;",
                '<' => "&lt;",
                '>' => "&gt;",
                '"' => "&quot;",
                '\'' => "&apos;",
                c if valid_xml(c) => c.encode_utf8(&mut utf8),
                _ => return Err(SkillError::InvalidSummary),
            };
            self.raw(text)?;
        }
        Ok(())
    }

    fn skill(
        &mut self,
        name: &str,
        description: impl Iterator<Item = Result<char, SkillError>>,
        location: &str,
    ) -> Result<(), SkillError> {
        self.raw("  <skill>\n    <name>")?;
        self.text(name.chars().map(Ok))?;
        self.raw("</name>\n    <description>")?;
        self.text(description)?;
        self.raw("</description>\n    <location>")?;
        self.text(location.chars().map(Ok))?;
        self.raw("</location>\n  </skill>\n")
    }
}

fn valid_xml(character: char) -> bool {
    matches!(character, '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}')
}
