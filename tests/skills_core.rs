use zharness::skills::{SkillError, SkillMetadata, SkillSummary, summaries_len, write_summaries};

fn description(source: &str) -> String {
    let metadata = SkillMetadata::parse(source).unwrap();
    let mut bytes = vec![0; metadata.description().len()];
    let text = metadata.description().write_to(&mut bytes).unwrap();
    assert_eq!(text, metadata.description().to_string());
    text.to_owned()
}

#[test]
fn metadata_is_borrowed_and_body_starts_after_frontmatter() {
    let source = "---\nname: rust-review\ndescription: Review Rust changes.\n---\n# Instructions\n";
    let skill = SkillMetadata::parse(source).unwrap();
    assert_eq!(skill.name(), "rust-review");
    assert_eq!(&source[skill.body_offset()..], "# Instructions\n");
    assert_eq!(skill.name().as_ptr(), source[10..].as_ptr());
    assert_eq!(description(source), "Review Rust changes.");
}

#[test]
fn bom_crlf_and_quoted_names_are_supported() {
    let source = "\u{feff}---\r\nname: 'rust-review' # portable name\r\ndescription: \"使用 Rust。\"\r\n---\r\nBody";
    let skill = SkillMetadata::parse(source).unwrap();
    assert_eq!(skill.name(), "rust-review");
    assert_eq!(description(source), "使用 Rust。");
    assert_eq!(&source[skill.body_offset()..], "Body");
}

#[test]
fn common_description_string_forms_normalize_whitespace() {
    let cases = [
        (
            "Use  this\n  when reviewing. # comment",
            "Use this when reviewing.",
        ),
        (
            "'Use it''s examples. # literal'",
            "Use it's examples. # literal",
        ),
        (
            r#""Read \"quoted\" paths \\ safely.""#,
            "Read \"quoted\" paths \\ safely.",
        ),
        (
            "|-\n  First line.\n  Second line.\n",
            "First line. Second line.",
        ),
        (
            ">- # synopsis\n  First line.\n\n  Second line.\n",
            "First line. Second line.",
        ),
        ("|2+\n  First\n  second", "First second"),
        ("'Several\n  lines'", "Several lines"),
        ("\"Several\n  lines\" # comment", "Several lines"),
        ("\"con\\\n  tinuation\"", "continuation"),
    ];
    for (value, expected) in cases {
        let source = format!("---\nname: review\ndescription: {value}\n---\n");
        assert_eq!(description(&source), expected, "{value:?}");
    }
}

#[test]
fn escapes_decode_unicode_without_splitting_utf8() {
    let source = r#"---
name: unicode
description: "\u5ba1\u67e5 \U0001F980 \x26\tRust\ncode"
---
"#;
    assert_eq!(description(source), "审查 🦀 & Rust code");
    let metadata = SkillMetadata::parse(source).unwrap();
    let needed = metadata.description().len();
    let mut short = vec![0xab; needed - 1];
    assert_eq!(
        metadata.description().write_to(&mut short),
        Err(SkillError::Capacity { needed })
    );
    assert!(short.iter().all(|byte| *byte == 0xab));
}

#[test]
fn nested_metadata_is_not_promoted_to_top_level_fields() {
    let source = "---\nmetadata:\n  name: wrong\n  description: wrong\nname: right\ndescription: Correct description.\nallowed-tools: Read Bash(git:*)\n---\n";
    let metadata = SkillMetadata::parse(source).unwrap();
    assert_eq!(metadata.name(), "right");
    assert_eq!(description(source), "Correct description.");
    assert_eq!(
        SkillMetadata::parse("---\nmetadata:\n  name: hidden\n  description: hidden\n---\n"),
        Err(SkillError::MissingName)
    );
}

#[test]
fn required_fields_and_frontmatter_errors_are_explicit() {
    let cases = [
        ("# Instructions", SkillError::MissingFrontmatter),
        ("---\nname: review\n", SkillError::UnterminatedFrontmatter),
        ("---\nname: review\n---", SkillError::MissingDescription),
        ("---\ndescription: Review.\n---", SkillError::MissingName),
        (
            "---\nname: review\nname: review\ndescription: Review.\n---",
            SkillError::DuplicateField,
        ),
        (
            "---\nname: review\ndescription: Review.\ndescription: Again.\n---",
            SkillError::DuplicateField,
        ),
        (
            "---\nname: review\ndescription: \"unclosed\n---",
            SkillError::InvalidFrontmatter,
        ),
        (
            "---\nname: review\ndescription: 'closed' garbage\n---",
            SkillError::InvalidFrontmatter,
        ),
        (
            "---\nname: review\ndescription: \"bad\\q\"\n---",
            SkillError::InvalidEscape,
        ),
        (
            "---\nname: review\ndescription: \"bad\\uD800\"\n---",
            SkillError::InvalidEscape,
        ),
        (
            "---\nname: review\ndescription: \"bad\\U00110000\"\n---",
            SkillError::InvalidEscape,
        ),
        (
            "---\nname: review\ndescription: \"bad\\UFFFFFFFF\"\n---",
            SkillError::InvalidEscape,
        ),
        (
            "---\nname: review\ndescription: \"bad\\0\"\n---",
            SkillError::InvalidDescription,
        ),
        (
            "---\nname: review\ndescription: &anchor text\n---",
            SkillError::UnsupportedScalar,
        ),
        (
            "---\nname: review\ndescription: [text]\n---",
            SkillError::UnsupportedScalar,
        ),
        (
            "---\nname: review\ndescription: {text: value}\n---",
            SkillError::UnsupportedScalar,
        ),
        (
            "---\nname: review\ndescription: !!str text\n---",
            SkillError::UnsupportedScalar,
        ),
        (
            "---\nname: review\ndescription:\n  nested: value\n---",
            SkillError::UnsupportedScalar,
        ),
        (
            "---\nname: review\ndescription:\n  - item\n---",
            SkillError::UnsupportedScalar,
        ),
        (
            "---\nname: review\ndescription: # comment\n  nested: value\n---",
            SkillError::UnsupportedScalar,
        ),
        (
            "---\nname: review\ndescription: |wrong\n  Text\n---",
            SkillError::UnsupportedScalar,
        ),
        (
            "---\nname: review\ndescription: '  '\n---",
            SkillError::InvalidDescription,
        ),
        (
            "---\nname: review\ndescription: |\n---",
            SkillError::InvalidDescription,
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(SkillMetadata::parse(source), Err(expected), "{source:?}");
    }
}

#[test]
fn portable_names_and_description_character_limits_are_enforced() {
    for name in [
        "",
        "Upper",
        "a--b",
        "-a",
        "a-",
        "a_b",
        "two words",
        "技能",
        "../a",
    ] {
        let source = format!("---\nname: '{name}'\ndescription: Text.\n---\n");
        assert_eq!(SkillMetadata::parse(&source), Err(SkillError::InvalidName));
    }
    for length in [64, 65] {
        let name = "a".repeat(length);
        let source = format!("---\nname: {name}\ndescription: Text.\n---\n");
        assert_eq!(SkillMetadata::parse(&source).is_ok(), length == 64);
    }
    for length in [1024, 1025] {
        let text = "🦀".repeat(length);
        let source = format!("---\nname: review\ndescription: {text}\n---\n");
        assert_eq!(SkillMetadata::parse(&source).is_ok(), length == 1024);
    }
}

#[test]
fn body_is_opaque_even_when_large_or_yaml_shaped() {
    let prefix = "---\nname: review\ndescription: Review.\n---\n";
    let mut source = String::from(prefix);
    source.push_str(&"name: INVALID\ndescription: [broken]\n\0\n".repeat(100_000));
    let skill = SkillMetadata::parse(&source).unwrap();
    assert_eq!(skill.body_offset(), prefix.len());
    assert_eq!(skill.name(), "review");
    assert_eq!(skill.description().len(), 7);
}

#[test]
fn summary_xml_is_escaped_and_atomic_on_all_input_errors() {
    let summaries = [SkillSummary {
        name: "rust-review",
        description: "Use <read> & \"review\" 'now'.",
        location: "/tmp/a&b/<skill>/SKILL.md",
    }];
    let expected = "<available_skills>\n  <skill>\n    <name>rust-review</name>\n    <description>Use &lt;read&gt; &amp; &quot;review&quot; &apos;now&apos;.</description>\n    <location>/tmp/a&amp;b/&lt;skill&gt;/SKILL.md</location>\n  </skill>\n</available_skills>\n";
    let needed = summaries_len(summaries).unwrap();
    assert_eq!(needed, expected.len());
    let mut output = vec![0x31; needed + 8];
    assert_eq!(write_summaries(summaries, &mut output).unwrap(), expected);
    assert_eq!(&output[needed..], &[0x31; 8]);
    let mut short = vec![0x31; needed - 1];
    assert_eq!(
        write_summaries(summaries, &mut short),
        Err(SkillError::Capacity { needed })
    );
    assert!(short.iter().all(|byte| *byte == 0x31));
    let invalid = SkillSummary {
        location: "bad\0path",
        ..summaries[0]
    };
    let mut output = [0x31; 1024];
    assert_eq!(
        write_summaries([summaries[0], invalid], &mut output),
        Err(SkillError::InvalidSummary)
    );
    assert_eq!(output, [0x31; 1024]);
}

#[test]
fn borrowed_metadata_summary_matches_normalized_catalog_summary() {
    let source =
        "---\nname: review\ndescription: >-\n  Review <Rust>\n  & tests.\n---\nInstructions";
    let metadata = SkillMetadata::parse(source).unwrap();
    let location = "repo/.agents/skills/review/SKILL.md";
    let mut buffer = vec![0; metadata.summary_len(location).unwrap()];
    let summary = metadata.write_summary(location, &mut buffer).unwrap();
    let normalized = description(source);
    let entry = SkillSummary {
        name: metadata.name(),
        description: &normalized,
        location,
    };
    let mut catalog = vec![0; summaries_len([entry]).unwrap()];
    assert_eq!(summary, write_summaries([entry], &mut catalog).unwrap());
    let mut short = [0xa5; 1];
    assert!(matches!(
        metadata.write_summary(location, &mut short),
        Err(SkillError::Capacity { .. })
    ));
    assert_eq!(short, [0xa5]);
}

#[test]
fn empty_catalog_has_an_explicit_container() {
    let mut buffer = [0; 64];
    assert_eq!(
        write_summaries([], &mut buffer).unwrap(),
        "<available_skills>\n</available_skills>\n"
    );
}
