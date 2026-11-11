//! Bounded MCP JSON-RPC primitives and a small MCP tools server.

use crate::{json::{self, Kind, Value}, tools::{Executor, ToolSet}};

/// MCP protocol version implemented by this server.
pub const PROTOCOL_VERSION: &str = "2025-11-25";

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
    /// Required method parameter is absent or invalid.
    InvalidParams,
    /// Tool is not registered.
    UnknownTool,
    /// Tool executor returned an error.
    ToolFailed,
}

/// Outcome of handling one complete JSON-RPC message.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// A response was written to output.
    Response(usize),
    /// A notification was accepted and requires no response.
    Notification,
}

/// Stateless MCP tools server. The caller owns framing, transport and buffers.
pub struct Server<'a> { name: &'a str, version: &'a str }
impl<'a> Server<'a> {
    /// Create a server identity.
    pub const fn new(name: &'a str, version: &'a str) -> Self { Self { name, version } }

    /// Handle one complete JSON-RPC message.
    ///
    /// `output` receives one JSON-RPC response without a trailing newline.
    /// `scratch` receives tool text before it is escaped into the response.
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
        let method = json::field(input, root, b"method")?.ok_or(Error::InvalidJson)?;
        let method = json::string(input, method)?;
        let id = json::field(input, root, b"id")?;
        if method == b"notifications/initialized" || method == b"notifications/cancelled" {
            return Ok(Outcome::Notification);
        }
        let Some(id) = id else { return Ok(Outcome::Notification); };
        let id_raw = id.bytes(input);
        let mut w = Writer::new(output);
        w.raw(b"{\"jsonrpc\":\"2.0\",\"id\":")?;
        w.raw(id_raw)?;
        w.raw(b",\"result\":")?;
        if method == b"initialize" {
            w.raw(b"{\"protocolVersion\":\"")?; w.raw(PROTOCOL_VERSION.as_bytes())?;
            w.raw(b"\",\"capabilities\":{\"tools\":{\"listChanged\":false}},\"serverInfo\":{\"name\":")?;
            w.quoted(self.name.as_bytes())?; w.raw(b",\"version\":")?; w.quoted(self.version.as_bytes())?; w.raw(b"}}")?;
        } else if method == b"ping" {
            w.raw(b"{}")?;
        } else if method == b"tools/list" {
            w.raw(b"{\"tools\":[")?;
            for (i, tool) in tools.as_slice().iter().enumerate() {
                if i != 0 { w.raw(b",")?; }
                w.raw(b"{\"name\":")?; w.quoted(tool.name.as_bytes())?;
                w.raw(b",\"description\":")?; w.quoted(tool.description.as_bytes())?;
                w.raw(b",\"inputSchema\":")?; w.raw(tool.input_schema.as_bytes())?; w.raw(b"}")?;
            }
            w.raw(b"]}")?;
        } else if method == b"tools/call" {
            let params = json::field(input, root, b"params")?.ok_or(Error::InvalidParams)?;
            let name = json::field(input, params, b"name")?.ok_or(Error::InvalidParams)?;
            let name = core::str::from_utf8(json::string(input, name)?).map_err(|_| Error::InvalidParams)?;
            if tools.get(name).is_none() { return write_error(output, id_raw, -32602, b"unknown tool").map(Outcome::Response); }
            let args = json::field(input, params, b"arguments")?.ok_or(Error::InvalidParams)?;
            if args.kind != Kind::Object { return write_error(output, id_raw, -32602, b"arguments must be an object").map(Outcome::Response); }
            let args_raw = args.bytes(input);
            let len = match executor.execute(name, args_raw, scratch) {
                Ok(n) if n <= scratch.len() => n,
                _ => return write_error(output, id_raw, -32000, b"tool execution failed").map(Outcome::Response),
            };
            w.raw(b"{\"content\":[{\"type\":\"text\",\"text\":")?; w.quoted(&scratch[..len])?; w.raw(b"}],\"isError\":false}")?;
        } else {
            return write_error(output, id_raw, -32601, b"method not found").map(Outcome::Response);
        }
        w.raw(b"}")?;
        Ok(Outcome::Response(w.len))
    }
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

fn write_error(output: &mut [u8], id: &[u8], code: i32, message: &[u8]) -> Result<usize, Error> {
    let mut w = Writer::new(output);
    w.raw(b"{\"jsonrpc\":\"2.0\",\"id\":")?; w.raw(id)?; w.raw(b",\"error\":{\"code\":")?;
    let mut digits = [0u8; 12]; let n = signed(code, &mut digits); w.raw(&digits[..n])?;
    w.raw(b",\"message\":")?; w.quoted(message)?; w.raw(b"}}")?; Ok(w.len)
}
fn signed(value: i32, out: &mut [u8; 12]) -> usize {
    let mut n = 0; let mut v = value.unsigned_abs(); if value < 0 { out[n] = b'-'; n += 1; }
    let start = n; loop { out[n] = b'0' + (v % 10) as u8; n += 1; v /= 10; if v == 0 { break; } }
    out[start..n].reverse(); n
}
struct Writer<'a> { out: &'a mut [u8], len: usize }
impl<'a> Writer<'a> {
    fn new(out: &'a mut [u8]) -> Self { Self { out, len: 0 } }
    fn raw(&mut self, bytes: &[u8]) -> Result<(), Error> { let end = self.len.checked_add(bytes.len()).ok_or(Error::Capacity)?; let dst = self.out.get_mut(self.len..end).ok_or(Error::Capacity)?; dst.copy_from_slice(bytes); self.len = end; Ok(()) }
    fn quoted(&mut self, bytes: &[u8]) -> Result<(), Error> {
        core::str::from_utf8(bytes).map_err(|_| Error::InvalidParams)?; self.raw(b"\"")?;
        for &b in bytes { match b { b'"' => self.raw(b"\\\"")?, b'\\' => self.raw(b"\\\\")?, b'\n' => self.raw(b"\\n")?, b'\r' => self.raw(b"\\r")?, b'\t' => self.raw(b"\\t")?, 0..=0x1f => { self.raw(b"\\u00")?; let hex = b"0123456789abcdef"; self.raw(&[hex[(b >> 4) as usize], hex[(b & 15) as usize]])?; }, _ => self.raw(&[b])? } }
        self.raw(b"\"")
    }
}
