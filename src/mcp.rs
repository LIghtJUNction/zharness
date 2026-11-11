//! Bounded MCP JSON-RPC tools server. The caller owns transport and all buffers.

use crate::{json::{self, Kind}, tools::{Executor, ToolSet}};

/// Current stable MCP protocol revision.
pub const PROTOCOL_VERSION: &str = "2026-07-28";
/// Latest handshake-era revision supported for backward compatibility.
pub const LEGACY_PROTOCOL_VERSION: &str = "2025-11-25";

/// MCP operation error. No error owns memory or formats dynamically.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Error {
    /// Input is not a valid supported JSON-RPC message.
    InvalidJson,
    /// JSON nesting exceeds the fixed scanner limit.
    DepthLimit,
    /// Output or scratch storage is too small.
    Capacity,
    /// Tool descriptor is invalid.
    InvalidTool,
    /// Tool names are duplicated.
    DuplicateTool,
    /// Method is unsupported.
    MethodNotFound,
    /// Required method parameters are absent or invalid.
    InvalidParams,
    /// Tool is not registered.
    UnknownTool,
    /// Tool execution failed.
    ToolFailed,
}


impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let message = match self {
            Self::InvalidJson => "invalid JSON",
            Self::DepthLimit => "JSON depth limit exceeded",
            Self::Capacity => "buffer capacity exceeded",
            Self::InvalidTool => "invalid tool descriptor",
            Self::DuplicateTool => "duplicate tool name",
            Self::MethodNotFound => "method not found",
            Self::InvalidParams => "invalid method parameters",
            Self::UnknownTool => "unknown tool",
            Self::ToolFailed => "tool execution failed",
        };
        f.write_str(message)
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}

/// Outcome of handling one complete JSON-RPC message.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// A response was written to output.
    Response(usize),
    /// A notification was accepted and requires no response.
    Notification,
}

/// Stateless MCP tools server. The caller owns framing, transport and buffers.
pub struct Server<'a> {
    name: &'a str,
    version: &'a str,
}

impl<'a> Server<'a> {
    /// Create a server identity.
    pub const fn new(name: &'a str, version: &'a str) -> Self {
        Self { name, version }
    }

    /// Handle one complete JSON-RPC message.
    ///
    /// The output receives one JSON-RPC response without a trailing newline.
    /// Scratch receives tool text before it is escaped into the response.
    /// The method is synchronous and never performs I/O.
    pub fn handle<E: Executor>(
        &self,
        input: &[u8],
        output: &mut [u8],
        scratch: &mut [u8],
        tools: &ToolSet<'_>,
        executor: &mut E,
    ) -> Result<Outcome, Error> {
        let root = json::root(input)?;
        let jsonrpc = json::field(input, root, b"jsonrpc")?.ok_or(Error::InvalidJson)?;
        if json::string(input, jsonrpc)? != b"2.0" {
            return write_error(output, b"null", -32600, b"invalid JSON-RPC version")
                .map(Outcome::Response);
        }
        let method = json::field(input, root, b"method")?.ok_or(Error::InvalidJson)?;
        let method = json::string(input, method)?;
        let id = json::field(input, root, b"id")?;

        if method.starts_with(b"notifications/") {
            if id.is_some() {
                return write_error(output, b"null", -32600, b"notification must not have an id")
                    .map(Outcome::Response);
            }
            return Ok(Outcome::Notification);
        }

        let Some(id) = id else {
            return write_error(output, b"null", -32600, b"request id is required")
                .map(Outcome::Response);
        };
        if matches!(id.kind, Kind::Object | Kind::Array) {
            return write_error(output, b"null", -32600, b"invalid request id")
                .map(Outcome::Response);
        }
        let id_raw = id.bytes(input);

        if method == b"initialize" {
            return self.initialize(input, root, id_raw, output).map(Outcome::Response);
        }
        if method == b"server/discover" {
            return self.discover(input, root, id_raw, output).map(Outcome::Response);
        }

        let modern = match request_version(input, root)? {
            Some(version) if version == PROTOCOL_VERSION.as_bytes() => true,
            Some(_) => {
                return write_error(output, id_raw, -32602, b"unsupported protocol version")
                    .map(Outcome::Response);
            }
            None => false,
        };

        if method == b"ping" {
            if modern {
                return write_error(output, id_raw, -32601, b"method not found")
                    .map(Outcome::Response);
            }
            return write_result(output, id_raw, b"{}").map(Outcome::Response);
        }
        if method == b"tools/list" {
            return self.tools_list(id_raw, modern, output, tools).map(Outcome::Response);
        }
        if method == b"tools/call" {
            return self.tools_call(input, root, id_raw, modern, output, scratch, tools, executor)
                .map(Outcome::Response);
        }
        write_error(output, id_raw, -32601, b"method not found").map(Outcome::Response)
    }

    fn initialize(
        &self,
        input: &[u8],
        root: json::Value,
        id: &[u8],
        output: &mut [u8],
    ) -> Result<usize, Error> {
        let params = json::field(input, root, b"params")?.ok_or(Error::InvalidParams)?;
        if params.kind != Kind::Object
            || json::field(input, params, b"protocolVersion")?.is_none()
        {
            return write_error(output, id, -32602, b"protocolVersion is required");
        }
        let mut w = Writer::new(output);
        w.raw(b"{\"jsonrpc\":\"2.0\",\"id\":")?;
        w.raw(id)?;
        w.raw(b",\"result\":{\"protocolVersion\":\"")?;
        w.raw(LEGACY_PROTOCOL_VERSION.as_bytes())?;
        w.raw(b"\",\"capabilities\":{\"tools\":{\"listChanged\":false}},\"serverInfo\":{\"name\":")?;
        w.quoted(self.name.as_bytes())?;
        w.raw(b",\"version\":")?;
        w.quoted(self.version.as_bytes())?;
        w.raw(b"}}}")?;
        Ok(w.len)
    }

    fn discover(
        &self,
        input: &[u8],
        root: json::Value,
        id: &[u8],
        output: &mut [u8],
    ) -> Result<usize, Error> {
        if request_version(input, root)? != Some(PROTOCOL_VERSION.as_bytes()) {
            return write_error(output, id, -32602, b"unsupported protocol version");
        }
        let mut w = Writer::new(output);
        w.raw(b"{\"jsonrpc\":\"2.0\",\"id\":")?;
        w.raw(id)?;
        w.raw(b",\"result\":{\"resultType\":\"complete\",\"supportedVersions\":[\"")?;
        w.raw(PROTOCOL_VERSION.as_bytes())?;
        w.raw(b"\",\"")?;
        w.raw(LEGACY_PROTOCOL_VERSION.as_bytes())?;
        w.raw(b"\"],\"capabilities\":{\"tools\":{}},\"_meta\":{\"io.modelcontextprotocol/serverInfo\":{\"name\":")?;
        w.quoted(self.name.as_bytes())?;
        w.raw(b",\"version\":")?;
        w.quoted(self.version.as_bytes())?;
        w.raw(b"}},\"ttlMs\":3600000,\"cacheScope\":\"public\"}}")?;
        Ok(w.len)
    }

    fn tools_list(
        &self,
        id: &[u8],
        modern: bool,
        output: &mut [u8],
        tools: &ToolSet<'_>,
    ) -> Result<usize, Error> {
        let mut w = Writer::new(output);
        w.raw(b"{\"jsonrpc\":\"2.0\",\"id\":")?;
        w.raw(id)?;
        if modern {
            w.raw(b",\"result\":{\"resultType\":\"complete\",\"tools\":[")?;
        } else {
            w.raw(b",\"result\":{\"tools\":[")?;
        }
        for (i, tool) in tools.as_slice().iter().enumerate() {
            if i != 0 { w.raw(b",")?; }
            w.raw(b"{\"name\":")?;
            w.quoted(tool.name.as_bytes())?;
            w.raw(b",\"description\":")?;
            w.quoted(tool.description.as_bytes())?;
            w.raw(b",\"inputSchema\":")?;
            w.raw(tool.input_schema.as_bytes())?;
            w.raw(b"}")?;
        }
        w.raw(b"]}}")?;
        Ok(w.len)
    }

    fn tools_call<E: Executor>(
        &self,
        input: &[u8],
        root: json::Value,
        id: &[u8],
        modern: bool,
        output: &mut [u8],
        scratch: &mut [u8],
        tools: &ToolSet<'_>,
        executor: &mut E,
    ) -> Result<usize, Error> {
        let params = json::field(input, root, b"params")?.ok_or(Error::InvalidParams)?;
        if params.kind != Kind::Object {
            return write_error(output, id, -32602, b"params must be an object");
        }
        let name = json::field(input, params, b"name")?.ok_or(Error::InvalidParams)?;
        let name = core::str::from_utf8(json::string(input, name)?)
            .map_err(|_| Error::InvalidParams)?;
        if tools.get(name).is_none() {
            return write_error(output, id, -32602, b"unknown tool");
        }
        let args = json::field(input, params, b"arguments")?.ok_or(Error::InvalidParams)?;
        if args.kind != Kind::Object {
            return write_error(output, id, -32602, b"arguments must be an object");
        }

        let (text, is_error) = match executor.execute(name, args.bytes(input), scratch) {
            Ok(n) if n <= scratch.len() => {
                if core::str::from_utf8(&scratch[..n]).is_err() {
                    (b"tool returned invalid UTF-8".as_slice(), true)
                } else {
                    (&scratch[..n], false)
                }
            }
            Ok(_) => return Err(Error::Capacity),
            Err(_) => (b"tool execution failed".as_slice(), true),
        };

        let mut w = Writer::new(output);
        w.raw(b"{\"jsonrpc\":\"2.0\",\"id\":")?;
        w.raw(id)?;
        w.raw(b",\"result\":{")?;
        if modern { w.raw(b"\"resultType\":\"complete\",")?; }
        w.raw(b"\"content\":[{\"type\":\"text\",\"text\":")?;
        w.quoted(text)?;
        w.raw(b"}],\"isError\":")?;
        w.raw(if is_error { b"true" } else { b"false" })?;
        w.raw(b"}}}")?;
        Ok(w.len)
    }
}

/// Extract the modern protocol version from params._meta. Missing metadata
/// denotes the legacy era; malformed metadata is rejected.
fn request_version(input: &[u8], root: json::Value) -> Result<Option<&[u8]>, Error> {
    let Some(params) = json::field(input, root, b"params")? else { return Ok(None); };
    if params.kind != Kind::Object { return Err(Error::InvalidParams); }
    let Some(meta) = json::field(input, params, b"_meta")? else { return Ok(None); };
    if meta.kind != Kind::Object { return Err(Error::InvalidParams); }
    let Some(version) = json::field(input, meta, b"io.modelcontextprotocol/protocolVersion")? else {
        return Ok(None);
    };
    Ok(Some(json::string(input, version)?))
}

/// Incremental newline-delimited MCP stdio framing without an internal buffer.
pub struct LineCodec;

impl LineCodec {
    /// Return the first complete line, excluding CR/LF. Partial input returns None.
    pub fn first(input: &[u8]) -> Option<&[u8]> {
        let end = input.iter().position(|&b| b == b'\n')?;
        let end = if end > 0 && input[end - 1] == b'\r' { end - 1 } else { end };
        Some(&input[..end])
    }
}

fn write_result(output: &mut [u8], id: &[u8], result: &[u8]) -> Result<usize, Error> {
    let mut w = Writer::new(output);
    w.raw(b"{\"jsonrpc\":\"2.0\",\"id\":")?;
    w.raw(id)?;
    w.raw(b",\"result\":")?;
    w.raw(result)?;
    w.raw(b"}")?;
    Ok(w.len)
}

fn write_error(output: &mut [u8], id: &[u8], code: i32, message: &[u8]) -> Result<usize, Error> {
    let mut w = Writer::new(output);
    w.raw(b"{\"jsonrpc\":\"2.0\",\"id\":")?;
    w.raw(id)?;
    w.raw(b",\"error\":{\"code\":")?;
    let mut digits = [0u8; 12];
    let n = signed(code, &mut digits);
    w.raw(&digits[..n])?;
    w.raw(b",\"message\":")?;
    w.quoted(message)?;
    w.raw(b"}}")?;
    Ok(w.len)
}

fn signed(value: i32, out: &mut [u8; 12]) -> usize {
    let mut n = 0;
    let mut v = value.unsigned_abs();
    if value < 0 { out[n] = b'-'; n += 1; }
    let start = n;
    loop {
        out[n] = b'0' + (v % 10) as u8;
        n += 1;
        v /= 10;
        if v == 0 { break; }
    }
    out[start..n].reverse();
    n
}

struct Writer<'a> {
    out: &'a mut [u8],
    len: usize,
}

impl<'a> Writer<'a> {
    fn new(out: &'a mut [u8]) -> Self { Self { out, len: 0 } }

    fn raw(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let end = self.len.checked_add(bytes.len()).ok_or(Error::Capacity)?;
        let dst = self.out.get_mut(self.len..end).ok_or(Error::Capacity)?;
        dst.copy_from_slice(bytes);
        self.len = end;
        Ok(())
    }

    fn quoted(&mut self, bytes: &[u8]) -> Result<(), Error> {
        core::str::from_utf8(bytes).map_err(|_| Error::InvalidParams)?;
        self.raw(b"\"")?;
        for &b in bytes {
            match b {
                b'"' => self.raw(b"\\\"")?,
                b'\\' => self.raw(b"\\\\")?,
                b'\n' => self.raw(b"\\n")?,
                b'\r' => self.raw(b"\\r")?,
                b'\t' => self.raw(b"\\t")?,
                0..=0x1f => {
                    self.raw(b"\\u00")?;
                    let hex = b"0123456789abcdef";
                    self.raw(&[hex[(b >> 4) as usize], hex[(b & 15) as usize]])?;
                }
                _ => self.raw(&[b])?,
            }
        }
        self.raw(b"\"")
    }
}
