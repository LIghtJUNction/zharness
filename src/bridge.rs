//! zcall ToolReady -> validated dispatch -> retained result -> next request.
//! No provider-specific fields, network calls, automatic retries or agent loops.
use crate::{
    Error,
    tools::{Executor, Registry},
    validation::{ArgumentLimits, Arguments, ValidationPolicy},
    zcall::v1::{
        Content, Data, EventData, Format, Presence, ProviderSignature, ReadyTool, Request, Session,
        Text, TokenStorage,
    },
};

/// Raw invocation view. It cannot be dispatched until `prepare` succeeds.
#[derive(Clone, Copy, Debug)]
pub struct Invocation<'a> {
    pub call_id: &'a str,
    pub name: &'a str,
    pub arguments: &'a [u8],
    pub signature: Option<ProviderSignature<'a>>,
}
impl<'a> Invocation<'a> {
    pub fn from_zcall(tool: ReadyTool<'a>) -> Self {
        Self {
            call_id: tool.call_id,
            name: tool.name,
            arguments: tool.arguments.as_bytes(),
            signature: tool.signature,
        }
    }
    pub fn from_event(event: EventData<'a>) -> Option<Self> {
        match event {
            EventData::ToolReady(tool) => Some(Self::from_zcall(tool)),
            _ => None,
        }
    }
    pub fn prepare<const N: usize, R: Registry>(
        &self,
        registry: &R,
        tokens: &mut TokenStorage<N>,
        limits: ArgumentLimits,
    ) -> Result<Prepared<'a>, Error> {
        if self.call_id.is_empty() {
            return Err(Error::InvalidArguments { offset: 0 });
        }
        if registry.find(self.name).is_none() {
            return Err(Error::UnknownTool);
        }
        let arguments = Arguments::parse(self.arguments, tokens, limits)?;
        Ok(Prepared {
            call_id: self.call_id,
            name: self.name,
            arguments,
            signature: self.signature,
        })
    }
}

/// Async boundary: copy only when a call must outlive the zcall event callback.
/// Storage is fixed, caller-owned, and can be placed in a caller-managed queue.
pub struct InvocationStorage<const N: usize> {
    bytes: [u8; N],
    id_len: usize,
    name_len: usize,
    args_len: usize,
    signature_source: Option<Format>,
    used: usize,
    occupied: bool,
}
impl<const N: usize> InvocationStorage<N> {
    pub const fn new() -> Self {
        Self {
            bytes: [0; N],
            id_len: 0,
            name_len: 0,
            args_len: 0,
            signature_source: None,
            used: 0,
            occupied: false,
        }
    }
    /// Capacity failure leaves the previous retained call intact.
    pub fn store(&mut self, call: Invocation<'_>) -> Result<(), Error> {
        let needed = call
            .call_id
            .len()
            .checked_add(call.name.len())
            .and_then(|n| n.checked_add(call.arguments.len()))
            .and_then(|n| n.checked_add(call.signature.map_or(0, |s| s.value.decoded_len())))
            .ok_or(Error::Capacity { needed: usize::MAX })?;
        if needed > N {
            return Err(Error::Capacity { needed });
        }
        let name_start = call.call_id.len();
        let args_start = name_start + call.name.len();
        self.bytes[..name_start].copy_from_slice(call.call_id.as_bytes());
        self.bytes[name_start..args_start].copy_from_slice(call.name.as_bytes());
        let args_end = args_start + call.arguments.len();
        self.bytes[args_start..args_end].copy_from_slice(call.arguments);
        if let Some(signature) = call.signature {
            signature
                .value
                .decode_into(&mut self.bytes[args_end..needed])?;
        }
        self.args_len = call.arguments.len();
        self.signature_source = call.signature.map(|signature| signature.source);
        self.id_len = name_start;
        self.name_len = call.name.len();
        self.used = needed;
        self.occupied = true;
        Ok(())
    }
    pub fn view(&self) -> Option<Invocation<'_>> {
        if !self.occupied {
            return None;
        }
        let args = self.id_len + self.name_len;
        Some(Invocation {
            call_id: core::str::from_utf8(&self.bytes[..self.id_len]).ok()?,
            name: core::str::from_utf8(&self.bytes[self.id_len..args]).ok()?,
            arguments: &self.bytes[args..args + self.args_len],
            signature: match self.signature_source {
                Some(source) => Some(ProviderSignature {
                    source,
                    value: Text::new(
                        core::str::from_utf8(&self.bytes[args + self.args_len..self.used]).ok()?,
                    ),
                }),
                None => None,
            },
        })
    }
    pub fn clear(&mut self) {
        self.occupied = false;
        self.used = 0;
    }
    pub const fn memory_bytes(&self) -> usize {
        core::mem::size_of::<Self>()
    }
}
impl<const N: usize> Default for InvocationStorage<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Prepared<'a> {
    call_id: &'a str,
    name: &'a str,
    arguments: Arguments<'a>,
    signature: Option<ProviderSignature<'a>>,
}
impl<'a> Prepared<'a> {
    /// Apply this provider-bound signature with Request::set_block_signature
    /// after appending content to the next conversation request.
    pub fn signature(self) -> Option<ProviderSignature<'a>> {
        self.signature
    }

    pub fn call_id(self) -> &'a str {
        self.call_id
    }
    pub fn name(self) -> &'a str {
        self.name
    }
    pub fn arguments(self) -> Arguments<'a> {
        self.arguments
    }
    pub fn check<P: ValidationPolicy>(self, policy: &mut P) -> Result<Self, Error> {
        policy.validate(self.name, self.arguments)?;
        Ok(self)
    }
    /// Append this content through zcall RequestBuilder::message(Role::Assistant, ...).
    /// Content excludes block metadata: retain signature() and apply it through
    /// Request::set_block_signature after appending a signed call.
    pub fn content(self) -> Content<'a> {
        Content::ToolCall {
            id: Presence::Value(Text::new(self.call_id)),
            name: Text::new(self.name),
            arguments: Data::Json(self.arguments.json()),
            legacy: false,
        }
    }
    /// Application must authorize before calling. This function never retries.
    pub fn execute<'o, E: Executor>(
        self,
        executor: &mut E,
        output: &'o mut [u8],
    ) -> Result<Completion<'a, 'o>, Error> {
        let result = executor.execute(self.name, self.arguments, output)?;
        let bytes = output.get(..result.written).ok_or(Error::InvalidOutput)?;
        let text = core::str::from_utf8(bytes).map_err(|_| Error::InvalidOutput)?;
        Ok(Completion {
            call_id: self.call_id,
            text,
            is_error: result.is_error,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Completion<'c, 'o> {
    pub call_id: &'c str,
    pub text: &'o str,
    pub is_error: bool,
}
impl<'c, 'o> Completion<'c, 'o> {
    /// The assistant's corresponding call must already exist in the request.
    /// zcall rejects unknown IDs and duplicate results without partial mutation.
    pub fn append_to<'r>(&self, request: &mut Request<'r, '_>) -> Result<(), Error>
    where
        'c: 'r,
        'o: 'r,
    {
        request.append_tool_result(
            self.call_id,
            Data::Text(Text::new(self.text)),
            if self.is_error {
                Presence::Value(true)
            } else {
                Presence::Missing
            },
        )?;
        Ok(())
    }
    /// Acknowledge only after the application has retained/accepted the result.
    /// This call changes zcall lifecycle state but does not perform a model call.
    pub fn submit(&self, session: &mut Session<'_, '_>) -> Result<(), Error> {
        session
            .submit_tool_result(self.call_id)
            .map_err(Error::from)
    }
}
