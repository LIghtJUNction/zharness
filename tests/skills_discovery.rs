#![cfg(feature = "std")]

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
use zharness::skills::{
    DiscoveryLimit, DiscoveryLimits, SkillDiagnosticKind, SkillLoadError, SkillPaths,
};

static NEXT_TEMP: AtomicUsize = AtomicUsize::new(0);

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        let path = env::temp_dir().join(format!(
            "zharness-skills-test-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn path(&self, relative: &str) -> PathBuf {
        self.0.join(relative)
    }
    fn write(&self, relative: &str, bytes: impl AsRef<[u8]>) -> PathBuf {
        let path = self.path(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        path
    }
    fn skill(&self, directory: &str, name: &str, description: &str, body: &str) -> PathBuf {
        self.write(
            &format!("{directory}/SKILL.md"),
            format!("---\nname: {name}\ndescription: {description}\n---\n{body}"),
        )
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn project_skills_overlay_global_skills_without_losing_unrelated_skills() {
    let temp = TempDir::new();
    let project = temp.path("project");
    let home = temp.path("home");
    let project_review = temp.skill(
        "project/.zharness/skills/review",
        "review",
        "Project review policy",
        "Use project rules.\n",
    );
    temp.skill(
        "project/.agents/skills/review",
        "review",
        "Shared project review",
        "Shared convention.\n",
    );
    temp.skill(
        "home/.zharness/skills/review",
        "review",
        "Global review policy",
        "Use general rules.\n",
    );
    temp.skill(
        "home/.agents/skills/explain",
        "explain",
        "Explain a program",
        "Start from inputs and outputs.\n",
    );

    let catalog = SkillPaths::for_project(&project, Some(&home)).discover();
    assert_eq!(catalog.len(), 2);
    assert_eq!(catalog.skills()[0].name(), "review");
    assert_eq!(catalog.skills()[1].name(), "explain");
    let review = catalog.get("review").unwrap();
    assert_eq!(review.description(), "Project review policy");
    assert_eq!(review.path(), fs::canonicalize(project_review).unwrap());
    assert_eq!(
        catalog.get("explain").unwrap().description(),
        "Explain a program"
    );
    assert!(catalog.get("absent").is_none());
    assert_eq!(
        catalog
            .diagnostics()
            .iter()
            .filter(|d| matches!(d.kind, SkillDiagnosticKind::Shadowed { .. }))
            .count(),
        2
    );
}

#[test]
fn search_path_order_controls_winners_and_preserves_default_fallbacks() {
    let temp = TempDir::new();
    temp.skill(
        "project/client tools/skills/review",
        "review",
        "Client rules",
        "Client.\n",
    );
    temp.skill(
        "project/other/skills/review",
        "review",
        "Other rules",
        "Other.\n",
    );
    temp.skill(
        "project/.agents/skills/local",
        "local",
        "Project-only skill",
        "Local.\n",
    );
    temp.skill(
        "home/.agents/skills/global",
        "global",
        "Global-only skill",
        "Global.\n",
    );
    let project = temp.path("project");
    let home = temp.path("home");

    for (first, second, winner) in [
        ("client tools/skills", "other/skills", "Client rules"),
        ("other/skills", "client tools/skills", "Other rules"),
    ] {
        let value = env::join_paths([Path::new(first), Path::new(""), Path::new(second)]).unwrap();
        let mut paths = SkillPaths::for_project(&project, Some(&home));
        paths.prepend_search_path(value, &project);
        assert_eq!(paths.roots()[0], project.join(first));
        assert_eq!(paths.roots()[1], project.join(second));
        assert!(!paths.roots().iter().any(|root| root.as_os_str().is_empty()));
        let catalog = paths.discover();
        assert_eq!(catalog.len(), 3);
        assert_eq!(catalog.skills()[0].description(), winner);
        assert!(catalog.get("local").is_some());
        assert!(catalog.get("global").is_some());
    }
}

#[test]
fn from_env_reads_skills_path_in_a_fresh_process() {
    const CHILD_PROJECT: &str = "ZHARNESS_SKILLS_TEST_PROJECT";
    if let Some(project) = env::var_os(CHILD_PROJECT) {
        let catalog = SkillPaths::from_env(PathBuf::from(project)).discover();
        assert_eq!(
            catalog.get("review").unwrap().description(),
            "Environment override"
        );
        assert!(catalog.get("project-only").is_some());
        assert!(catalog.get("global-only").is_some());
        return;
    }

    let temp = TempDir::new();
    temp.skill(
        "project/app skills/review",
        "review",
        "Environment override",
        "Custom.\n",
    );
    temp.skill(
        "project/.agents/skills/review",
        "review",
        "Project default",
        "Default.\n",
    );
    temp.skill(
        "project/.agents/skills/project-only",
        "project-only",
        "Project skill",
        "Project.\n",
    );
    temp.skill(
        "home/.agents/skills/global-only",
        "global-only",
        "Global skill",
        "Global.\n",
    );
    let output = Command::new(env::current_exe().unwrap())
        .args([
            "--exact",
            "from_env_reads_skills_path_in_a_fresh_process",
            "--nocapture",
        ])
        .env(CHILD_PROJECT, temp.path("project"))
        .env(
            "SKILLS_PATH",
            env::join_paths([Path::new("app skills")]).unwrap(),
        )
        .env("HOME", temp.path("home"))
        .env("USERPROFILE", temp.path("home"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn explicit_custom_directories_and_prepend_push_have_predictable_order() {
    let temp = TempDir::new();
    temp.skill("first/check", "check", "First", "First.\n");
    temp.skill("second/check", "check", "Second", "Second.\n");
    temp.skill("last/extra", "extra", "Extra", "Last.\n");
    let mut paths = SkillPaths::new([temp.path("second")]);
    paths.prepend(temp.path("first")).push(temp.path("last"));
    assert_eq!(
        paths.roots(),
        &[temp.path("first"), temp.path("second"), temp.path("last")]
    );
    let catalog = paths.discover();
    assert_eq!(catalog.len(), 2);
    assert_eq!(catalog.get("check").unwrap().description(), "First");
    assert!(catalog.get("extra").is_some());
}

#[test]
fn discovery_is_sorted_and_stops_at_each_skill_directory() {
    let temp = TempDir::new();
    temp.skill("skills/z-last", "review", "Last alphabetically", "Last.\n");
    temp.skill(
        "skills/a-first",
        "review",
        "First alphabetically",
        "First.\n",
    );
    temp.skill(
        "skills/bundle/nested",
        "nested",
        "Grouped skill",
        "Nested.\n",
    );
    temp.skill(
        "skills/a-first/references/not-a-skill",
        "hidden-in-skill",
        "Reference fixture",
        "Ignored.\n",
    );
    temp.skill(
        "skills/.hidden/not-visible",
        "hidden-directory",
        "Hidden fixture",
        "Ignored.\n",
    );

    let catalog = SkillPaths::new([temp.path("skills")]).discover();
    assert_eq!(
        catalog
            .skills()
            .iter()
            .map(|s| s.name())
            .collect::<Vec<_>>(),
        ["review", "nested"]
    );
    assert_eq!(
        catalog.get("review").unwrap().description(),
        "First alphabetically"
    );
    assert!(catalog.get("hidden-in-skill").is_none());
    assert!(catalog.get("hidden-directory").is_none());
}

#[test]
fn malformed_higher_priority_files_do_not_reserve_names() {
    let temp = TempDir::new();
    temp.write(
        "bad/review/SKILL.md",
        "---\nname: review\n---\nMissing description.\n",
    );
    temp.skill("valid/review", "review", "Working fallback", "Working.\n");
    let catalog =
        SkillPaths::new([temp.path("missing"), temp.path("bad"), temp.path("valid")]).discover();
    assert_eq!(catalog.len(), 1);
    assert_eq!(
        catalog.get("review").unwrap().description(),
        "Working fallback"
    );
    assert!(
        catalog
            .diagnostics()
            .iter()
            .any(|d| matches!(d.kind, SkillDiagnosticKind::InvalidMetadata(_)))
    );
    assert!(
        !catalog
            .diagnostics()
            .iter()
            .any(|d| matches!(d.kind, SkillDiagnosticKind::Shadowed { .. }))
    );
}

#[test]
fn duplicate_paths_are_deduplicated_before_registration() {
    let temp = TempDir::new();
    temp.skill("skills/review", "review", "One physical skill", "Once.\n");
    let root = temp.path("skills");
    let catalog = SkillPaths::new([root.clone(), root.join("."), root]).discover();
    assert_eq!(catalog.len(), 1);
    assert!(catalog.diagnostics().is_empty());
}

#[test]
fn explicitly_listed_roots_receive_their_own_depth_budget() {
    let temp = TempDir::new();
    temp.skill(
        "skills/group/review",
        "review",
        "Explicit nested root",
        "Review.\n",
    );
    let catalog = SkillPaths::new([temp.path("skills"), temp.path("skills/group")])
        .discover_with_limits(DiscoveryLimits {
            max_depth: 1,
            ..DiscoveryLimits::default()
        });
    assert_eq!(
        catalog.get("review").unwrap().description(),
        "Explicit nested root"
    );
    assert_eq!(catalog.len(), 1);
}

#[test]
fn platform_search_path_quoting_keeps_directory_names_intact() {
    let temp = TempDir::new();
    // A semicolon is a valid filename character and a Windows PATH separator.
    temp.skill(
        "project/client;tools/skills/review",
        "review",
        "Quoted path",
        "Review.\n",
    );
    let value = env::join_paths([Path::new("client;tools/skills")]).unwrap();
    let mut paths = SkillPaths::default();
    paths.prepend_search_path(value, temp.path("project"));
    assert_eq!(
        paths.discover().get("review").unwrap().description(),
        "Quoted path"
    );
}

#[cfg(unix)]
#[test]
fn symlink_aliases_and_cycles_do_not_duplicate_skills_or_recurse_forever() {
    use std::os::unix::fs::symlink;
    let temp = TempDir::new();
    temp.skill("skills/review", "review", "Canonical skill", "Once.\n");
    symlink(temp.path("skills"), temp.path("alias")).unwrap();
    symlink(temp.path("skills"), temp.path("skills/loop")).unwrap();
    let catalog = SkillPaths::new([temp.path("alias"), temp.path("skills")]).discover();
    assert_eq!(catalog.len(), 1);
    assert!(catalog.diagnostics().is_empty());
}

#[test]
fn summaries_are_utf8_escaped_and_capacity_failure_is_atomic() {
    let temp = TempDir::new();
    temp.skill(
        "skills/review",
        "review",
        "'分析 <xml> & \"quotes\" safely.'",
        "SECRET BODY MARKER\n",
    );
    let catalog = SkillPaths::new([temp.path("skills")]).discover();
    let mut output = [0; 4096];
    let summary = catalog.write_summary(&mut output).unwrap();
    assert!(summary.starts_with("<available_skills>"));
    assert!(summary.contains("<name>review</name>"));
    assert!(summary.contains("分析 &lt;xml&gt; &amp; &quot;quotes&quot; safely."));
    assert!(!summary.contains("SECRET BODY MARKER"));
    assert!(summary.contains("SKILL.md"));
    let entry = catalog.get("review").unwrap();
    assert_eq!(entry.summary().name, "review");
    assert_eq!(entry.summary().description, entry.description());
    let mut too_small = [b'x'; 16];
    assert!(catalog.write_summary(&mut too_small).is_err());
    assert_eq!(too_small, [b'x'; 16]);
}

#[test]
fn large_bodies_are_lazy_and_activation_is_explicitly_bounded() {
    let temp = TempDir::new();
    let body = "large body line\n".repeat(80_000);
    let file = temp.skill("skills/large", "large", "Small metadata", &body);
    let catalog = SkillPaths::new([temp.path("skills")]).discover_with_limits(DiscoveryLimits {
        max_metadata_bytes: 128,
        ..DiscoveryLimits::default()
    });
    assert!(catalog.diagnostics().is_empty());
    let entry = catalog.get("large").unwrap();
    assert_eq!(entry.description(), "Small metadata");
    assert!(matches!(
        entry.load(1024),
        Err(SkillLoadError::LimitExceeded { limit: 1024 })
    ));
    let exact_bytes = fs::metadata(&file).unwrap().len() as usize;
    let loaded = entry.load(exact_bytes).unwrap();
    assert_eq!(loaded.body(), body);
    assert_eq!(loaded.source().len(), exact_bytes);
    assert_eq!(
        loaded.directory(),
        fs::canonicalize(file.parent().unwrap()).unwrap()
    );
}

#[test]
fn malformed_body_bytes_are_not_read_as_discovery_metadata() {
    let temp = TempDir::new();
    let mut file = b"---\nname: binary-body\ndescription: Valid header\n---\n".to_vec();
    file.extend_from_slice(&[0xff, 0xfe, 0xfd]);
    temp.write("skills/binary-body/SKILL.md", file);
    let catalog = SkillPaths::new([temp.path("skills")]).discover();
    assert_eq!(catalog.len(), 1);
    assert!(catalog.diagnostics().is_empty());
    assert!(catalog.get("binary-body").unwrap().load(1024).is_err());
}

#[test]
fn changed_metadata_requires_rediscovery_before_activation() {
    let temp = TempDir::new();
    let path = temp.skill(
        "skills/review",
        "review",
        "Original scope",
        "Original body.\n",
    );
    let catalog = SkillPaths::new([temp.path("skills")]).discover();
    temp.skill(
        "skills/review",
        "review",
        "Different scope",
        "Changed body.\n",
    );
    assert!(matches!(
        catalog.get("review").unwrap().load(1024),
        Err(SkillLoadError::Changed)
    ));
    let refreshed = SkillPaths::new([temp.path("skills")]).discover();
    assert_eq!(
        refreshed.get("review").unwrap().load(1024).unwrap().body(),
        "Changed body.\n"
    );
    fs::remove_file(path).unwrap();
    assert!(matches!(
        refreshed.get("review").unwrap().load(1024),
        Err(SkillLoadError::Io(_))
    ));
}

#[test]
fn resources_are_bounded_and_confined_to_the_selected_skill() {
    let temp = TempDir::new();
    temp.skill(
        "skills/review",
        "review",
        "Review with references",
        "Read references/rules.txt.\n",
    );
    temp.write("skills/review/references/rules.txt", "rule one\n");
    let outside = temp.write("outside.txt", "outside\n");
    let catalog = SkillPaths::new([temp.path("skills")]).discover();
    let entry = catalog.get("review").unwrap();
    assert_eq!(
        entry.read_resource("references/rules.txt", 9).unwrap(),
        b"rule one\n"
    );
    assert!(matches!(
        entry.read_resource("references/rules.txt", 8),
        Err(SkillLoadError::LimitExceeded { limit: 8 })
    ));
    assert!(matches!(
        entry.read_resource("../../outside.txt", 100),
        Err(SkillLoadError::InvalidResource)
    ));
    assert!(matches!(
        entry.read_resource(&outside, 100),
        Err(SkillLoadError::InvalidResource)
    ));
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside, temp.path("skills/review/references/escape.txt"))
            .unwrap();
        assert!(matches!(
            entry.read_resource("references/escape.txt", 100),
            Err(SkillLoadError::InvalidResource)
        ));
    }
}

#[test]
fn discovery_limits_report_partial_catalogs_instead_of_silent_truncation() {
    let temp = TempDir::new();
    temp.skill("skills/first", "first", "First skill", "First.\n");
    temp.skill("skills/group/deep", "deep", "Grouped skill", "Deep.\n");
    temp.skill("skills/last", "last", "Last skill", "Last.\n");
    let paths = SkillPaths::new([temp.path("skills")]);
    let shallow = paths.discover_with_limits(DiscoveryLimits {
        max_depth: 1,
        ..DiscoveryLimits::default()
    });
    assert!(shallow.get("first").is_some());
    assert!(shallow.get("deep").is_none());
    assert!(shallow.diagnostics().iter().any(|d| matches!(
        d.kind,
        SkillDiagnosticKind::LimitReached(DiscoveryLimit::Depth)
    )));

    for (limits, expected) in [
        (
            DiscoveryLimits {
                max_skills: 1,
                ..DiscoveryLimits::default()
            },
            DiscoveryLimit::Skills,
        ),
        (
            DiscoveryLimits {
                max_entries: 1,
                ..DiscoveryLimits::default()
            },
            DiscoveryLimit::Entries,
        ),
    ] {
        let catalog = paths.discover_with_limits(limits);
        assert!(catalog.len() <= 1);
        assert!(catalog.diagnostics().iter().any(
            |d| matches!(d.kind, SkillDiagnosticKind::LimitReached(kind) if kind == expected)
        ));
    }

    temp.skill("oversized/large", "large", &"d".repeat(200), "Body.\n");
    let oversized =
        SkillPaths::new([temp.path("oversized")]).discover_with_limits(DiscoveryLimits {
            max_metadata_bytes: 64,
            ..DiscoveryLimits::default()
        });
    assert!(oversized.is_empty());
    assert!(oversized.diagnostics().iter().any(|d| matches!(
        d.kind,
        SkillDiagnosticKind::LimitReached(DiscoveryLimit::MetadataBytes)
    )));
}
