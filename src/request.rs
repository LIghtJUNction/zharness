//! Hook the actual complete-message send boundary without owning network I/O.
//! An accepted send is not evidence of provider receipt or model generation.
use crate::{
    Error,
    hooks::HookResult,
    lifecycle::{
        Correlation, Fault, Gate, Lifecycle, Observer, Payload,
        domains::request::{Action, Event},
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SendReceipt {
    pub accepted_bytes: usize,
}
/// The adapter defines acceptance (for example, queued locally or written).
/// A partial write/error may have external effects; never auto-retry blindly.
pub trait Sender {
    fn send(&mut self, message: &[u8]) -> Result<SendReceipt, Fault>;
}
pub fn send_with_hooks<'a, G, O, S: Sender>(
    correlation: Correlation,
    bytes: &'a [u8],
    sender: &mut S,
    lifecycle: &mut Lifecycle<G, O>,
) -> Result<HookResult<SendReceipt>, Fault>
where
    G: Gate<Event<'a>>,
    O: Observer<Event<'a>>,
{
    if correlation.request.is_none() {
        return Err(Error::InvalidArguments { offset: 0 }.into());
    }
    let event = Event::new(Action::Send).with_payload(Payload::bytes(bytes));
    lifecycle.run(correlation, event, |_| {
        let receipt = sender.send(bytes)?;
        if receipt.accepted_bytes != bytes.len() {
            return Err(Error::InvalidOutput.into());
        }
        Ok(receipt)
    })
}
