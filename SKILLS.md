# Skills

zharness provides metadata parsing, ordered discovery, compact catalogs and
on-demand loading for `SKILL.md` skills. The core borrows its input and writes
into caller-owned buffers. The optional `std` adapter reads environment variables
and local files, retaining metadata rather than every skill's instructions.

A skill summary means its declared name, normalized description and source
location. Generating it requires no model call and does not infer a semantic
summary from the Markdown body. Applications decide which skill to activate,
where to place its instructions, and which tools may execute them.

## Parse and summarize without I/O

The `skills` core is available with `default-features = false`; it needs neither
`std` nor `alloc` and adds no third-party dependency. Keep the source string alive
while using its borrowed metadata.

```rust
use zharness::skills::{SkillMetadata, SkillSummary, write_summaries};
# fn main() -> Result<(), zharness::skills::SkillError> {
let source = "---\nname: review\ndescription: >-\n  Review code changes.\n  Use before a merge.\n---\n# Review\nRead the diff first.\n";
let metadata = SkillMetadata::parse(source)?;
let mut description_buffer = [0; 1024];
let description = metadata.description().write_to(&mut description_buffer)?;
assert_eq!(description, "Review code changes. Use before a merge.");

let mut output = [0; 1024];
let summary = write_summaries(
    core::iter::once(SkillSummary {
        name: metadata.name(),
        description,
        location: "/project/.agents/skills/review/SKILL.md",
    }),
    &mut output,
)?;
assert!(summary.contains("<name>review</name>"));
assert!(source[metadata.body_offset()..].starts_with("# Review"));
# Ok(())
# }
```

`SkillMetadata::parse` stops at the closing frontmatter delimiter. Its
`body_offset()` points immediately after that delimiter's line; parsing does not
walk the remaining instructions. `Description::len()` reports normalized UTF-8
bytes, and `Display` or `write_to` decodes the description without an intermediate
owned string. Allocation by a destination such as `format!` remains the caller's
choice.

For a single entry, `metadata.write_summary(location, output)` avoids a separate
description buffer. For a collection, use `write_summaries` and a repeatable,
cloneable iterator of `SkillSummary` entries. `summaries_len` and
`metadata.summary_len(location)` return the exact required buffer capacity.
Summary rendering escapes XML text and returns a complete `<available_skills>`
catalog. Invalid XML characters or insufficient capacity return an error;
capacity errors report `needed` and preserve the output buffer.

## Discover project, global and custom skills

The `std` feature is enabled by default. Its types are re-exported directly from
`zharness::skills`, including `SkillPaths`, `SkillCatalog` and `DiscoveryLimits`.

```rust,no_run
# #[cfg(feature = "std")]
# fn main() -> Result<(), Box<dyn std::error::Error>> {
use zharness::skills::SkillPaths;

let mut paths = SkillPaths::from_env("/workspace/project");
paths.push("/opt/editor/skills");
let catalog = paths.discover();
for diagnostic in catalog.diagnostics() {
    eprintln!("{diagnostic:?}");
}

let mut output = vec![0; catalog.summary_len()?];
let summary = catalog.write_summary(&mut output)?;
// Supply `summary` to your model using your own prompt/context policy.
println!("{summary}");

if let Some(entry) = catalog.get("review") {
    let loaded = entry.load(256 * 1024)?;
    println!("{}\n{}", loaded.directory().display(), loaded.body());
    // Read a resource only when the selected skill needs it:
    // let guide = entry.read_resource("references/review.md", 64 * 1024)?;
}
# Ok(())
# }
# #[cfg(not(feature = "std"))]
# fn main() {}
```

`SkillPaths::for_project(project, home)` uses these roots, in priority order:

| Priority | Root |
| --- | --- |
| 1 | `<project>/.zharness/skills` |
| 2 | `<project>/.agents/skills` |
| 3 | `<home>/.zharness/skills` |
| 4 | `<home>/.agents/skills` |

Pass `None` for `home` to omit global roots. The application supplies the project
root; zharness does not search ancestors or infer a Git root. A typical skill is
`<root>/review/SKILL.md`. A root may also point directly at a skill directory.

Discovery combines distinct skill names across roots. For a duplicate name, the
first valid definition wins and the later definition produces a `Shadowed`
diagnostic containing the winner's path. Thus the default order lets a project
replace a same-named global skill while retaining other global skills. The
library selects one complete definition; it does not concatenate their bodies.

`prepend(root)` gives a directory highest priority. `push(root)` gives it lowest
priority. Both accept an application's own skills directory, including locations
used by another client. Identical roots are deduplicated, and `roots()` exposes
the final ordered search plan. Explicit relative roots use the process working
directory.

## Control order with SKILLS_PATH

`SkillPaths::from_env(project)` reads `SKILLS_PATH` once, prepends its entries in
left-to-right order, then appends the four project/global defaults. It reads
`HOME` on Unix and `USERPROFILE` on Windows for the global roots. Entries earlier
in the resulting list win name collisions, including collisions with defaults.

```sh
export SKILLS_PATH="$PWD/.agents/skills:$HOME/.claude/skills:/opt/editor/skills"
```

On Unix the separator is `:`. On Windows it is `;`, using the platform's PATH
quoting rules. `prepend_search_path(value, relative_to)` applies the same parsing
to an explicit `OsStr`, without reading the process environment. Relative
environment entries are anchored at `project`; empty entries are ignored.
An unset or empty `SKILLS_PATH` leaves defaults intact.

No shell, tilde, environment-variable or glob expansion occurs inside the
library. In the shell example above, the shell expands `$PWD` and `$HOME` before
zharness receives the value. Prefer absolute paths for configuration shared
between applications with different working directories.

Use `SkillPaths::new(roots)` for an exact, ordered set of directories without
defaults or environment reads. Use `SkillPaths::default()` for an empty plan.
For example, `SkillPaths::new(["/opt/editor/skills"])` discovers only that source.
This keeps an application's explicit configuration independent of ambient
`SKILLS_PATH` settings.

## Discovery and loading boundaries

The filesystem adapter sorts child paths before scanning them and preserves
discovery order in `catalog.skills()`. It recursively finds grouping directories
but treats a directory containing `SKILL.md` as a leaf, including when that
file's metadata is invalid. Hidden child directories and `node_modules` are
skipped. Symlink targets are canonicalized to prevent cycles and duplicate file
registration. An explicitly listed root receives its full depth budget even if
an earlier, broader root already reached that directory with less depth left.

Missing roots are skipped. Read errors, invalid metadata, non-UTF-8 skill paths,
collisions and reached limits are available through `catalog.diagnostics()`.
An invalid high-priority definition does not reserve its name: a valid definition
in a later root can still load. Review diagnostics when showing an incomplete
catalog to a user.

`discover_with_limits` accepts `DiscoveryLimits`. Defaults are grouping depth 8,
4,096 skills, 65,536 inspected directory entries and 65,536 frontmatter bytes per
file. The entry limit counts ignored files too. Reaching a limit produces a
diagnostic instead of silently promising a complete search.

Discovery reads bounded frontmatter and stores names, normalized descriptions
and canonical source paths. It does not retain bodies or read bundled resources.
The filesystem adapter allocates metadata and traversal storage; the core's
allocation-free contract does not cover filesystem discovery.

`catalog.get(name)` uses the retained metadata index without I/O.
`catalog.summary_len()` reports the exact caller-buffer size for `write_summary`.
`entry.load(max_bytes)` reads the selected UTF-8 file with an explicit size cap
and returns its full `source()`, exact `body()` and base `directory()`. Body-only
edits appear on the next load. If its name or normalized description changed,
loading returns `SkillLoadError::Changed`; rediscover to update the catalog.

`entry.read_resource(relative, max_bytes)` loads bytes relative to the selected
skill directory. It rejects absolute paths, parent traversal and symlinks that
resolve outside that directory. These checks are path guards for trusted local
files, not an operating-system sandbox against concurrent hostile filesystem
changes. Discovery, summary generation and loading do not execute scripts or
grant tool permissions.

## Supported metadata syntax

The parser implements the string forms needed for skill metadata, not a complete
YAML processor. Required fields are top-level `name` and `description` between
`---` delimiter lines. UTF-8 BOM, LF and CRLF are accepted.

| Field or form | Behavior |
| --- | --- |
| `name` | 1–64 lowercase ASCII letters, digits or single hyphens; no leading/trailing hyphen |
| `description` | 1–1,024 Unicode scalar values after whitespace normalization |
| Plain text | Supports indented continuations and whitespace-delimited comments |
| Single-quoted text | Decodes doubled single quotes |
| Double-quoted text | Decodes supported YAML character escapes, including Unicode escapes |
| `|` and `>` block text | Accepts chomp/indent indicators; collapses whitespace for the summary |
| Duplicate required keys | Error; no ambiguous winner inside one file |
| Aliases, tags and flow collections in required fields | Unsupported; returns an error |
| Other top-level keys | Skipped with their indented contents |

Descriptions normalize whitespace to a single space regardless of scalar style;
they are routing summaries, not a lossless YAML representation. Raw instructions
remain available through the original source. Names use a portable ASCII subset,
and quoted names cannot require escape decoding. The declared `name` is the
lookup key; matching it to the containing directory remains advisable for
interoperability, but zharness does not enforce that match.

Fields such as `allowed-tools`, `disable-model-invocation`, hooks and
client-specific metadata do not acquire execution or permission semantics merely
by being present. Applications that use those extensions must implement their
policy explicitly.

## Reproduce core measurements

```sh
cargo bench --locked --bench skills
ZHARNESS_BENCH_SMOKE=1 cargo bench --locked --bench skills
cargo test --locked --no-default-features
```

The benchmark covers plain and multiline 256-byte headers, the same headers with
a 1 MiB body, description rendering, and catalogs of 1, 16, 64 and 256 skills.
The paired inputs expose any body-length-dependent parsing cost. Catalog timing
includes the capacity/validation pass and the write pass.

Fixtures, output buffers and timing storage are allocated before measurement.
Each case asserts zero allocations inside its measured operations. CSV `bytes`
means input bytes for parsing and rendered bytes for output cases. `mean_ns` uses
a batch; percentile samples include the clock's instrumentation overhead.
Smoke mode checks execution and allocation assertions with fewer iterations; it
is not a stable performance comparison. No filesystem, process startup or model
latency is included, and no fixed latency is promised across machines.

## Design references

- [Agent Skills specification](https://agentskills.io/specification): portable
  `SKILL.md` structure and metadata-first loading.
- [Agent Skills integration guide](https://agentskills.io/client-implementation/adding-skills-support):
  discovery, compact catalogs, diagnostics and activation.
- [OpenCode skills](https://opencode.ai/docs/skills/): multiple sources and
  on-demand instructions; its registration order differs from zharness.
- [Rust `split_paths`](https://doc.rust-lang.org/std/env/fn.split_paths.html):
  platform PATH separators and Windows quoting.

The Agent Skills format does not standardize search roots or `SKILLS_PATH`.
zharness's first-valid-wins order and environment behavior are the contract
described above, rather than an attempt to reproduce every client's precedence.
