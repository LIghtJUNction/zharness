//! Borrowed skill metadata plus optional filesystem discovery. No model or network.
use zharness::skills::{SkillMetadata, SkillSummary, write_summaries};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = "---\nname: review\ndescription: Review code against project rules.\n---\nCheck behavior before style.\n";
    let metadata = SkillMetadata::parse(source)?;
    let mut description = [0; 256];
    let description = metadata.description().write_to(&mut description)?;
    let summary = SkillSummary {
        name: metadata.name(),
        description,
        location: "embedded/review/SKILL.md",
    };
    let mut prompt = [0; 2048];
    println!("{}", write_summaries([summary], &mut prompt)?);
    assert_eq!(
        &source[metadata.body_offset()..],
        "Check behavior before style.\n"
    );

    #[cfg(feature = "std")]
    discover_files()?;
    Ok(())
}

#[cfg(feature = "std")]
fn discover_files() -> Result<(), Box<dyn std::error::Error>> {
    use std::{fs, path::PathBuf};
    use zharness::skills::SkillPaths;

    struct Fixture(PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let fixture = Fixture(std::env::temp_dir().join(format!(
        "zharness-skills-example-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos()
    )));
    let project = fixture.0.join("project");
    let home = fixture.0.join("home");
    let custom = fixture.0.join("editor/skills");
    for (directory, name, description, body) in [
        (
            project.join(".agents/skills/review"),
            "review",
            "Project review rules",
            "Check project invariants.\n",
        ),
        (
            home.join(".agents/skills/review"),
            "review",
            "Global review rules",
            "Check general conventions.\n",
        ),
        (
            home.join(".agents/skills/explain"),
            "explain",
            "Explain a design",
            "Start from inputs and outputs.\n",
        ),
        (
            custom.join("editor"),
            "editor",
            "Editor-specific assistance",
            "Use the editor's commands.\n",
        ),
    ] {
        fs::create_dir_all(&directory)?;
        fs::write(
            directory.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {description}\n---\n{body}"),
        )?;
    }
    let mut paths = SkillPaths::for_project(&project, Some(&home));
    // Use from_env(project) in a host application to read SKILLS_PATH directly.
    // An explicit value makes this example independent of the user's environment.
    paths.prepend_search_path(std::env::join_paths([&custom])?, &project);
    let catalog = paths.discover();
    assert_eq!(catalog.len(), 3);
    assert_eq!(
        catalog.get("review").unwrap().description(),
        "Project review rules"
    );
    let mut prompt = [0; 4096];
    println!("{}", catalog.write_summary(&mut prompt)?);
    let selected = catalog.get("review").unwrap().load(64 * 1024)?;
    assert_eq!(selected.body(), "Check project invariants.\n");
    println!("Selected instructions:\n{}", selected.body());
    Ok(())
}
