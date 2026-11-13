//! Static/fixed-capacity tool metadata and caller-owned execution.
use crate::{
    Error,
    validation::Arguments,
    zcall::v1::{Json, JsonKind, Limits, Presence, Text, TokenStorage, ToolDefinition},
};

pub(crate) fn validate_name(name: &str) -> Result<(), Error> {
    if name.is_empty()
        || name.len() > 128
        || !name.is_ascii()
        || name.bytes().any(|b| b.is_ascii_control() || b == b' ')
    {
        Err(Error::InvalidTool)
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Tool<'a> {
    pub name: &'a str,
    pub description: &'a str,
    pub input_schema: &'a str,
}
impl<'a> Tool<'a> {
    pub const fn new(name: &'a str, description: &'a str, input_schema: &'a str) -> Self {
        Self {
            name,
            description,
            input_schema,
        }
    }
    pub fn validate(&self) -> Result<(), Error> {
        self.validate_with(&mut TokenStorage::<128>::new())
            .map(|_| ())
    }
    pub fn validate_with<const N: usize>(
        &self,
        tokens: &mut TokenStorage<N>,
    ) -> Result<Json<'a>, Error> {
        validate_name(self.name)?;
        let schema = Json::parse(self.input_schema.as_bytes(), tokens, Limits::default())?;
        if schema.kind() != JsonKind::Object {
            return Err(Error::InvalidTool);
        }
        Ok(schema)
    }
    /// Reuse zcall's public definition type; no provider JSON is generated here.
    pub fn as_zcall<const N: usize>(
        &self,
        tokens: &mut TokenStorage<N>,
    ) -> Result<ToolDefinition<'a>, Error> {
        Ok(ToolDefinition {
            name: Text::new(self.name),
            description: Presence::Value(Text::new(self.description)),
            parameters: Presence::Value(self.validate_with(tokens)?),
            schema_dialect: crate::zcall::v1::SchemaDialect::JsonSchema,
            strict: Presence::Missing,
            legacy: false,
        })
    }
}

pub trait Registry {
    fn find(&self, name: &str) -> Option<&Tool<'_>>;
}

#[derive(Clone, Copy)]
pub struct ToolSet<'a> {
    tools: &'a [Tool<'a>],
}
impl<'a> ToolSet<'a> {
    /// Convenience construction uses a bounded 128-token temporary. Use
    /// with_storage for larger schemas or application-controlled workspace.
    pub fn new(tools: &'a [Tool<'a>]) -> Result<Self, Error> {
        Self::with_storage(tools, &mut TokenStorage::<128>::new())
    }
    pub fn with_storage<const N: usize>(
        tools: &'a [Tool<'a>],
        tokens: &mut TokenStorage<N>,
    ) -> Result<Self, Error> {
        for (i, tool) in tools.iter().enumerate() {
            tool.validate_with(tokens)?;
            if tools[..i].iter().any(|prior| prior.name == tool.name) {
                return Err(Error::DuplicateTool);
            }
        }
        Ok(Self { tools })
    }
    pub fn as_slice(self) -> &'a [Tool<'a>] {
        self.tools
    }
    pub fn get(self, name: &str) -> Option<&'a Tool<'a>> {
        self.tools.iter().find(|t| t.name == name)
    }
    pub const fn len(self) -> usize {
        self.tools.len()
    }
    pub const fn is_empty(self) -> bool {
        self.tools.is_empty()
    }
    /// Build a sorted name index without reordering descriptors or allocating.
    /// Linear lookup remains available for small registries; selection is explicit.
    pub fn indexed<'s>(self, storage: &'s mut [usize]) -> Result<IndexedTools<'a, 's>, Error> {
        let index = storage.get_mut(..self.tools.len()).ok_or(Error::Capacity {
            needed: self.tools.len(),
        })?;
        for (i, slot) in index.iter_mut().enumerate() {
            *slot = i;
        }
        index.sort_unstable_by(|a, b| self.tools[*a].name.cmp(self.tools[*b].name));
        Ok(IndexedTools {
            tools: self.tools,
            index,
        })
    }
}
impl Registry for ToolSet<'_> {
    fn find(&self, name: &str) -> Option<&Tool<'_>> {
        self.get(name)
    }
}

pub struct IndexedTools<'a, 's> {
    tools: &'a [Tool<'a>],
    index: &'s [usize],
}
impl<'a> IndexedTools<'a, '_> {
    pub fn get(&self, name: &str) -> Option<&'a Tool<'a>> {
        self.index
            .binary_search_by(|i| self.tools[*i].name.cmp(name))
            .ok()
            .map(|i| &self.tools[self.index[i]])
    }
    pub fn memory_bytes(&self) -> usize {
        core::mem::size_of::<Self>() + core::mem::size_of_val(self.index)
    }
}
impl Registry for IndexedTools<'_, '_> {
    fn find(&self, name: &str) -> Option<&Tool<'_>> {
        self.get(name)
    }
}

pub struct FixedRegistry<'a, 's> {
    slots: &'s mut [Option<Tool<'a>>],
    used: usize,
}
impl<'a, 's> FixedRegistry<'a, 's> {
    /// Initializes the caller's slots to empty.
    pub fn new(slots: &'s mut [Option<Tool<'a>>]) -> Self {
        slots.fill(None);
        Self { slots, used: 0 }
    }
    pub fn insert<const N: usize>(
        &mut self,
        tool: Tool<'a>,
        tokens: &mut TokenStorage<N>,
    ) -> Result<(), Error> {
        tool.validate_with(tokens)?;
        if self.find(tool.name).is_some() {
            return Err(Error::DuplicateTool);
        }
        if self.used == self.slots.len() {
            return Err(Error::Capacity {
                needed: self.used.saturating_add(1),
            });
        }
        self.slots[self.used] = Some(tool);
        self.used += 1;
        Ok(())
    }
    pub fn len(&self) -> usize {
        self.used
    }
    pub fn is_empty(&self) -> bool {
        self.used == 0
    }
    pub fn memory_bytes(&self) -> usize {
        core::mem::size_of::<Self>() + core::mem::size_of_val(self.slots)
    }
}
impl Registry for FixedRegistry<'_, '_> {
    fn find(&self, name: &str) -> Option<&Tool<'_>> {
        self.slots[..self.used]
            .iter()
            .flatten()
            .find(|tool| tool.name == name)
    }
}

/// Describes bytes already retained in the output buffer. A business failure is
/// a normal completed tool result with is_error=true, not a transport error.
#[derive(Clone, Copy, Debug)]
pub struct ToolOutput {
    pub written: usize,
    pub is_error: bool,
}
impl ToolOutput {
    pub const fn success(written: usize) -> Self {
        Self {
            written,
            is_error: false,
        }
    }
    pub const fn failure(written: usize) -> Self {
        Self {
            written,
            is_error: true,
        }
    }
}

pub trait Executor {
    /// Called only by the application, after its authorization decision.
    /// Arguments are a complete validated object. Output must be UTF-8 text.
    /// A capacity error is not permission to automatically replay side effects.
    fn execute(
        &mut self,
        name: &str,
        arguments: Arguments<'_>,
        output: &mut [u8],
    ) -> Result<ToolOutput, Error>;
}

/// Compatibility import for the model invocation view.
pub use crate::bridge::Invocation;
