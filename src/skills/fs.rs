//! Explicit filesystem and environment adapter; available with `std`.
extern crate std;

use super::{SkillError, SkillMetadata, SkillSummary, write_summaries};
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsStr,
    fs::{self, File},
    io::{self, BufRead, BufReader, Read},
    path::{Component, Path, PathBuf},
    string::{String, ToString},
    vec::Vec,
};

/// Platform-delimited directory list, searched from left to right before defaults.
pub const SKILLS_PATH: &str = "SKILLS_PATH";

/// Ordered search roots. Earlier valid skills win name collisions.
#[derive(Clone, Debug, Default)]
pub struct SkillPaths {
    roots: Vec<PathBuf>,
}

impl SkillPaths {
    /// Use exactly these roots, without reading environment variables or defaults.
    /// Relative paths are interpreted against the process working directory.
    pub fn new<P: Into<PathBuf>>(roots: impl IntoIterator<Item = P>) -> Self {
        let mut paths = Self::default();
        for root in roots {
            paths.push(root);
        }
        paths
    }

    /// Project-native, project-shared, user-native, then user-shared directories.
    /// Pass the project root explicitly; no ancestor or Git-root search occurs.
    pub fn for_project(project_root: impl AsRef<Path>, home: Option<&Path>) -> Self {
        let mut paths = Self::default();
        for base in [Some(project_root.as_ref()), home].into_iter().flatten() {
            paths.push(base.join(".zharness/skills"));
            paths.push(base.join(".agents/skills"));
        }
        paths
    }

    /// Read `SKILLS_PATH` once and prepend it to the project/user defaults.
    /// An unset or empty variable leaves the defaults intact. Use `new` for a
    /// complete replacement, or `default` for an empty search plan.
    pub fn from_env(project_root: impl AsRef<Path>) -> Self {
        #[cfg(windows)]
        let home = env::var_os("USERPROFILE");
        #[cfg(not(windows))]
        let home = env::var_os("HOME");
        let home = home.filter(|value| !value.is_empty()).map(PathBuf::from);
        let mut paths = Self::for_project(project_root.as_ref(), home.as_deref());
        if let Some(value) = env::var_os(SKILLS_PATH) {
            paths.prepend_search_path(value, project_root);
        }
        paths
    }

    /// Parse the host PATH syntax (`:` on Unix, `;` on Windows). Empty segments
    /// are ignored; relative entries are anchored at `relative_to`. No shell,
    /// variable, tilde or glob expansion occurs.
    pub fn prepend_search_path(
        &mut self,
        value: impl AsRef<OsStr>,
        relative_to: impl AsRef<Path>,
    ) -> &mut Self {
        let mut ordered = Self::default();
        for root in env::split_paths(value.as_ref()).filter(|root| !root.as_os_str().is_empty()) {
            ordered.push(if root.is_absolute() {
                root
            } else {
                relative_to.as_ref().join(root)
            });
        }
        for root in self.roots.drain(..) {
            ordered.push(root);
        }
        self.roots = ordered.roots;
        self
    }

    /// Give a root highest priority, moving an identical existing root forward.
    pub fn prepend(&mut self, root: impl Into<PathBuf>) -> &mut Self {
        let root = root.into();
        if !root.as_os_str().is_empty() {
            self.roots.retain(|existing| *existing != root);
            self.roots.insert(0, root);
        }
        self
    }

    /// Append a lowest-priority root. Exact duplicates and empty roots are ignored.
    pub fn push(&mut self, root: impl Into<PathBuf>) -> &mut Self {
        let root = root.into();
        if !root.as_os_str().is_empty() && !self.roots.contains(&root) {
            self.roots.push(root);
        }
        self
    }

    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    pub fn discover(&self) -> SkillCatalog {
        self.discover_with_limits(DiscoveryLimits::default())
    }

    pub fn discover_with_limits(&self, limits: DiscoveryLimits) -> SkillCatalog {
        let mut scan = Scanner {
            catalog: SkillCatalog::default(),
            limits,
            directories: BTreeMap::new(),
            files: BTreeSet::new(),
            entries: 0,
            stopped: false,
            header: Vec::new(),
        };
        for root in &self.roots {
            if scan.stopped {
                break;
            }
            scan.visit(root, 0, true);
        }
        scan.catalog
    }
}

#[derive(Clone, Copy, Debug)]
pub struct DiscoveryLimits {
    /// Maximum grouping depth below a search root (root depth is zero).
    pub max_depth: usize,
    pub max_skills: usize,
    /// Total directory entries inspected across all roots, including ignored files.
    pub max_entries: usize,
    /// Maximum frontmatter bytes, including delimiters; independent of body size.
    pub max_metadata_bytes: usize,
}

impl Default for DiscoveryLimits {
    fn default() -> Self {
        Self {
            max_depth: 8,
            max_skills: 4096,
            max_entries: 65_536,
            max_metadata_bytes: 65_536,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiscoveryLimit {
    Depth,
    Skills,
    Entries,
    MetadataBytes,
}

#[derive(Debug)]
pub struct SkillDiagnostic {
    pub path: PathBuf,
    pub kind: SkillDiagnosticKind,
}

#[derive(Debug)]
#[non_exhaustive]
pub enum SkillDiagnosticKind {
    Io(io::ErrorKind),
    InvalidUtf8,
    NonUtf8Path,
    InvalidMetadata(SkillError),
    LimitReached(DiscoveryLimit),
    Shadowed { name: String, winner: PathBuf },
}

/// Metadata only, retained in discovery order. No skill bodies are cached.
#[derive(Debug, Default)]
pub struct SkillCatalog {
    skills: Vec<SkillEntry>,
    // Sorted indices avoid a second owned copy of every skill name.
    index: Vec<usize>,
    diagnostics: Vec<SkillDiagnostic>,
}

impl SkillCatalog {
    pub fn skills(&self) -> &[SkillEntry] {
        &self.skills
    }
    pub fn len(&self) -> usize {
        self.skills.len()
    }
    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }
    pub fn diagnostics(&self) -> &[SkillDiagnostic] {
        &self.diagnostics
    }
    fn find(&self, name: &str) -> Result<usize, usize> {
        self.index
            .binary_search_by(|&index| self.skills[index].name.as_str().cmp(name))
    }
    /// Binary search over the index; no filesystem I/O or allocation.
    pub fn get(&self, name: &str) -> Option<&SkillEntry> {
        self.find(name).ok().map(|i| &self.skills[self.index[i]])
    }
    /// Render the whole metadata catalog into caller storage, without I/O.
    pub fn write_summary<'o>(&self, output: &'o mut [u8]) -> Result<&'o str, SkillError> {
        write_summaries(self.skills.iter().map(SkillEntry::summary), output)
    }
}

#[derive(Debug)]
pub struct SkillEntry {
    name: String,
    description: String,
    // Always canonical and UTF-8, checked before registration.
    path: PathBuf,
}

impl SkillEntry {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn description(&self) -> &str {
        &self.description
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn summary(&self) -> SkillSummary<'_> {
        SkillSummary {
            name: &self.name,
            description: &self.description,
            location: self.path.to_str().expect("registered UTF-8 path"),
        }
    }

    /// Load a selected skill on demand. Re-discover if its name or normalized
    /// description changed; body-only edits are visible on the next load.
    pub fn load(&self, max_bytes: usize) -> Result<LoadedSkill, SkillLoadError> {
        let bytes = read_bounded(&self.path, max_bytes)?;
        let source = String::from_utf8(bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let metadata = SkillMetadata::parse(&source).map_err(SkillLoadError::InvalidMetadata)?;
        if metadata.name() != self.name
            || !metadata
                .description()
                .chars()
                .eq(self.description.chars().map(Ok))
        {
            return Err(SkillLoadError::Changed);
        }
        let body_offset = metadata.body_offset();
        Ok(LoadedSkill {
            source,
            body_offset,
            directory: self
                .path
                .parent()
                .expect("canonical file parent")
                .to_path_buf(),
        })
    }

    /// Read a resource relative to this skill, with a byte cap. Absolute paths,
    /// parent traversal and symlinks escaping the skill directory are rejected.
    /// This is a path guard for trusted local files, not an OS sandbox.
    pub fn read_resource(
        &self,
        relative: impl AsRef<Path>,
        max_bytes: usize,
    ) -> Result<Vec<u8>, SkillLoadError> {
        let relative = relative.as_ref();
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
        {
            return Err(SkillLoadError::InvalidResource);
        }
        let base = self.path.parent().expect("canonical file parent");
        let path = fs::canonicalize(base.join(relative))?;
        if !path.starts_with(base) {
            return Err(SkillLoadError::InvalidResource);
        }
        read_bounded(&path, max_bytes)
    }
}

#[derive(Debug)]
pub struct LoadedSkill {
    source: String,
    body_offset: usize,
    directory: PathBuf,
}

impl LoadedSkill {
    pub fn source(&self) -> &str {
        &self.source
    }
    pub fn body(&self) -> &str {
        &self.source[self.body_offset..]
    }
    pub fn directory(&self) -> &Path {
        &self.directory
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum SkillLoadError {
    Io(io::Error),
    InvalidMetadata(SkillError),
    LimitExceeded { limit: usize },
    Changed,
    InvalidResource,
}

impl From<io::Error> for SkillLoadError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
impl core::fmt::Display for SkillLoadError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl core::error::Error for SkillLoadError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::InvalidMetadata(error) => Some(error),
            _ => None,
        }
    }
}

fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, SkillLoadError> {
    let file = File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "expected a regular file").into());
    }
    let mut bytes = Vec::new();
    file.take((limit as u64).saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(SkillLoadError::LimitExceeded { limit });
    }
    Ok(bytes)
}

struct Scanner {
    catalog: SkillCatalog,
    limits: DiscoveryLimits,
    directories: BTreeMap<PathBuf, usize>,
    files: BTreeSet<PathBuf>,
    entries: usize,
    stopped: bool,
    header: Vec<u8>,
}

impl Scanner {
    fn diagnostic(&mut self, path: &Path, kind: SkillDiagnosticKind) {
        self.catalog.diagnostics.push(SkillDiagnostic {
            path: path.to_path_buf(),
            kind,
        });
    }
    fn limit(&mut self, path: &Path, limit: DiscoveryLimit) {
        self.diagnostic(path, SkillDiagnosticKind::LimitReached(limit));
    }
    fn visit(&mut self, path: &Path, depth: usize, root: bool) {
        let canonical = match fs::canonicalize(path) {
            Ok(path) => path,
            Err(error) => {
                if !root || error.kind() != io::ErrorKind::NotFound {
                    self.diagnostic(path, SkillDiagnosticKind::Io(error.kind()));
                }
                return;
            }
        };
        let remaining_depth = self.limits.max_depth - depth;
        if self
            .directories
            .get(&canonical)
            .is_some_and(|&scanned_depth| scanned_depth >= remaining_depth)
        {
            return;
        }
        self.directories.insert(canonical.clone(), remaining_depth);
        let skill = canonical.join("SKILL.md");
        match fs::metadata(&skill) {
            Ok(metadata) => {
                if metadata.is_file() {
                    self.register(&skill);
                } else {
                    self.diagnostic(&skill, SkillDiagnosticKind::Io(io::ErrorKind::InvalidInput));
                }
                // A skill is a leaf even when its metadata is invalid.
                return;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                self.diagnostic(&skill, SkillDiagnosticKind::Io(error.kind()));
                return;
            }
        }
        let entries = match fs::read_dir(&canonical) {
            Ok(entries) => entries,
            Err(error) => {
                self.diagnostic(&canonical, SkillDiagnosticKind::Io(error.kind()));
                return;
            }
        };
        let mut children = Vec::new();
        for entry in entries {
            if self.entries >= self.limits.max_entries {
                self.limit(&canonical, DiscoveryLimit::Entries);
                self.stopped = true;
                return;
            }
            self.entries += 1;
            match entry {
                Ok(entry) => {
                    let name = entry.file_name();
                    if name.to_string_lossy().starts_with('.') || name == "node_modules" {
                        continue;
                    }
                    match entry.file_type() {
                        Ok(kind) if kind.is_dir() || kind.is_symlink() => {
                            children.push(entry.path())
                        }
                        Ok(_) => {}
                        Err(error) => {
                            self.diagnostic(&entry.path(), SkillDiagnosticKind::Io(error.kind()));
                        }
                    }
                }
                Err(error) => self.diagnostic(&canonical, SkillDiagnosticKind::Io(error.kind())),
            }
        }
        children.sort_unstable();
        for child in children {
            if self.stopped {
                break;
            }
            if depth >= self.limits.max_depth {
                self.limit(&child, DiscoveryLimit::Depth);
                continue;
            }
            self.visit(&child, depth + 1, false);
        }
    }

    fn register(&mut self, path: &Path) {
        let path = match fs::canonicalize(path) {
            Ok(path) => path,
            Err(error) => {
                self.diagnostic(path, SkillDiagnosticKind::Io(error.kind()));
                return;
            }
        };
        if !self.files.insert(path.clone()) {
            return;
        }
        if path.to_str().is_none() {
            self.diagnostic(&path, SkillDiagnosticKind::NonUtf8Path);
            return;
        }
        if let Err(kind) = self.read_header(&path) {
            self.diagnostic(&path, kind);
            return;
        }
        let header = match core::str::from_utf8(&self.header) {
            Ok(header) => header,
            Err(_) => {
                self.diagnostic(&path, SkillDiagnosticKind::InvalidUtf8);
                return;
            }
        };
        let metadata = match SkillMetadata::parse(header) {
            Ok(metadata) => metadata,
            Err(error) => {
                self.diagnostic(&path, SkillDiagnosticKind::InvalidMetadata(error));
                return;
            }
        };
        match self.catalog.find(metadata.name()) {
            Ok(index) => {
                let winner = &self.catalog.skills[self.catalog.index[index]];
                let kind = SkillDiagnosticKind::Shadowed {
                    name: metadata.name().to_string(),
                    winner: winner.path.clone(),
                };
                self.diagnostic(&path, kind);
            }
            Err(index) => {
                if self.catalog.len() >= self.limits.max_skills {
                    self.limit(&path, DiscoveryLimit::Skills);
                    self.stopped = true;
                    return;
                }
                let skill = SkillEntry {
                    name: metadata.name().to_string(),
                    description: metadata.description().to_string(),
                    path,
                };
                self.catalog.index.insert(index, self.catalog.skills.len());
                self.catalog.skills.push(skill);
            }
        }
    }

    fn read_header(&mut self, path: &Path) -> Result<(), SkillDiagnosticKind> {
        let file = File::open(path).map_err(|error| SkillDiagnosticKind::Io(error.kind()))?;
        let mut reader = BufReader::with_capacity(1024, file)
            .take((self.limits.max_metadata_bytes as u64).saturating_add(1));
        self.header.clear();
        loop {
            let start = self.header.len();
            let bytes = reader
                .read_until(b'\n', &mut self.header)
                .map_err(|error| SkillDiagnosticKind::Io(error.kind()))?;
            if self.header.len() > self.limits.max_metadata_bytes {
                return Err(SkillDiagnosticKind::LimitReached(
                    DiscoveryLimit::MetadataBytes,
                ));
            }
            if bytes == 0 {
                return Ok(()); // Parser reports missing or unterminated metadata.
            }
            let line = &self.header[start..];
            let line = line.strip_suffix(b"\n").unwrap_or(line);
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            let line = line.trim_ascii_end();
            if start > 0 && line == b"---" {
                return Ok(());
            }
            if start == 0 && line.strip_prefix(b"\xef\xbb\xbf").unwrap_or(line) != b"---" {
                return Ok(());
            }
        }
    }
}
