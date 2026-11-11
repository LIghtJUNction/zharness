//! Static tool descriptions and caller-owned execution interfaces.

use crate::mcp::Error;

/// One MCP tool descriptor. The schema must be a complete JSON object.
#[derive(Clone, Copy, Debug)]
pub struct Tool<'a> {
    /// Stable tool name.
    pub name: &'a str,
    /// Human-readable description.
    pub description: &'a str,
    /// JSON Schema object encoded as UTF-8 JSON.
    pub input_schema: &'a str,
}
impl<'a> Tool<'a> {
    /// Create a static tool descriptor.
    pub const fn new(name: &'a str, description: &'a str, input_schema: &'a str) -> Self {
        Self { name, description, input_schema }
    }

    /// Validate the descriptor's basic wire constraints.
    pub fn validate(&self) -> Result<(), Error> {
        if self.name.is_empty() || self.name.len() > 128 || !self.name.is_ascii() || self.name.bytes().any(|b| b.is_ascii_control() || b == b' ') { return Err(Error::InvalidTool); }
        let schema = crate::json::root(self.input_schema.as_bytes()).map_err(|_| Error::InvalidTool)?;
        if schema.kind != crate::json::Kind::Object {
            return Err(Error::InvalidTool);
        }
        Ok(())
    }
}

/// A borrowed, validated tool call from zcall's normalized stream.
#[derive(Clone, Copy, Debug)]
pub struct Invocation<'a> {
    /// Provider-independent call identifier.
    pub call_id: &'a str,
    /// Registered tool name.
    pub name: &'a str,
    /// Complete JSON object arguments.
    pub arguments: &'a [u8],
}
impl<'a> Invocation<'a> {
    /// Adapt a complete zcall ToolReady event without copying its arguments.
    pub fn from_zcall(tool: crate::zcall::v1::ReadyTool<'a>) -> Self {
        Self { call_id: tool.call_id, name: tool.name, arguments: tool.arguments.as_bytes() }
    }
}

/// Executes a tool into a caller-provided UTF-8 text buffer.
///
/// Return the number of bytes written. The result is exposed to MCP as a text
/// content block; the executor must not write JSON framing around the text.
pub trait Executor {
    /// Execute one already-authorized tool invocation.
    fn execute(&mut self, name: &str, arguments: &[u8], output: &mut [u8]) -> Result<usize, Error>;
}

/// Static tool registry. No allocation, hashing or runtime registration.
pub struct ToolSet<'a> { tools: &'a [Tool<'a>] }
impl<'a> ToolSet<'a> {
    /// Construct and validate a static registry.
    pub fn new(tools: &'a [Tool<'a>]) -> Result<Self, Error> {
        for (i, tool) in tools.iter().enumerate() {
            tool.validate()?;
            if tools[..i].iter().any(|prior| prior.name == tool.name) { return Err(Error::DuplicateTool); }
        }
        Ok(Self { tools })
    }
    /// Return all descriptors in declaration order.
    pub fn as_slice(&self) -> &'a [Tool<'a>] { self.tools }
    /// Find a tool by exact name.
    pub fn get(&self, name: &str) -> Option<&'a Tool<'a>> { self.tools.iter().find(|tool| tool.name == name) }
    /// Number of registered tools.
    pub const fn len(&self) -> usize { self.tools.len() }
    /// Whether the registry is empty.
    pub const fn is_empty(&self) -> bool { self.tools.is_empty() }
}
