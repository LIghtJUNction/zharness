//! Transport contracts and caller-buffered line framing.

/// Transport failure category. Concrete adapters may map their native errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum TransportError {
    /// The peer or stream has closed cleanly.
    Closed,
    /// Caller-provided storage cannot hold the next complete message.
    Capacity,
    /// The underlying transport failed.
    Io,
    /// A message boundary or frame is invalid.
    InvalidFrame,
    /// The previous complete frame has not been consumed.
    FramePending,
}

/// Result of reading one complete message.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadStatus {
    /// A complete message was written to the supplied buffer.
    Message(usize),
    /// The transport reached a clean end of stream.
    Closed,
}

/// Reads one complete message into caller-owned storage.
pub trait MessageReader {
    /// Read a complete message. A capacity error must not silently truncate it.
    fn read_message(&mut self, output: &mut [u8]) -> Result<ReadStatus, TransportError>;
}

/// Writes one complete message. Implementations must handle partial native writes.
pub trait MessageWriter {
    /// Write the complete message or return an error.
    fn write_message(&mut self, message: &[u8]) -> Result<(), TransportError>;
}

/// A bidirectional message transport.
pub trait Transport: MessageReader + MessageWriter {}

impl<T: MessageReader + MessageWriter> Transport for T {}

/// A completed line in the caller-provided frame buffer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LineFrame {
    /// Bytes in the frame excluding CR/LF.
    pub len: usize,
    /// Total bytes consumed from the backing buffer, including the delimiter.
    pub framed_len: usize,
}

/// Incremental newline frame decoder. It never allocates or reads from a stream.
pub struct LineDecoder<'a> {
    buffer: &'a mut [u8],
    used: usize,
    frame: Option<LineFrame>,
}

impl<'a> LineDecoder<'a> {
    /// Use caller-provided storage for incremental input.
    pub const fn new(buffer: &'a mut [u8]) -> Self {
        Self {
            buffer,
            used: 0,
            frame: None,
        }
    }

    /// Append input until one line is complete or the input is exhausted.
    /// Returns the number of bytes consumed from input.
    pub fn push(&mut self, input: &[u8]) -> Result<usize, TransportError> {
        if self.frame.is_some() {
            return Err(TransportError::FramePending);
        }
        let mut consumed = 0;
        for &byte in input {
            let Some(slot) = self.buffer.get_mut(self.used) else {
                return Err(TransportError::Capacity);
            };
            *slot = byte;
            self.used += 1;
            consumed += 1;
            if byte == b'\n' {
                let cr = self.used >= 2 && self.buffer[self.used - 2] == b'\r';
                let framed_len = self.used;
                let len = self.used - if cr { 2 } else { 1 };
                self.frame = Some(LineFrame { len, framed_len });
                break;
            }
        }
        Ok(consumed)
    }

    /// Borrow the completed message without its line delimiter.
    pub fn message(&self) -> Option<&[u8]> {
        self.frame.map(|frame| &self.buffer[..frame.len])
    }

    /// Consume the completed frame and retain any bytes after it.
    pub fn consume(&mut self) -> Result<(), TransportError> {
        let frame = self.frame.take().ok_or(TransportError::InvalidFrame)?;
        self.buffer.copy_within(frame.framed_len..self.used, 0);
        self.used -= frame.framed_len;
        Ok(())
    }

    /// Bytes currently retained in the caller's buffer.
    pub const fn buffered_len(&self) -> usize {
        self.used
    }

    /// Total caller-provided capacity.
    pub const fn capacity(&self) -> usize {
        self.buffer.len()
    }

    /// Number of bytes still available.
    pub const fn remaining(&self) -> usize {
        self.buffer.len() - self.used
    }
}
