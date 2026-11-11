//! Complete local conversation/configuration/send/response/persistence lifecycle.
//! Uses a captured real zcall stream, not a live provider or hidden network client.
use zharness::{
    Error,
    context::{Budget, Context, Group, Message, Overflow},
    conversation::Conversation,
    hooks::{Decision, HookResult},
    lifecycle::{
        Correlation, Event, Fault, Gate, Lifecycle, NoGate, NoObserver, Notice, Observer, Outcome,
        Payload, Phase, Proposal,
        domains::{conversation, model, persistence, request, response},
    },
    request::{SendReceipt, Sender, send_with_hooks},
    response::ResponseStream,
    settings::ModelSettings,
    zcall::v1::*,
};

#[derive(Default)]
struct MetadataAudit {
    started: usize,
    finished: usize,
}
impl<E: Event> Observer<E> for MetadataAudit {
    fn observe(&mut self, event: Notice<E>) {
        match event.phase {
            Phase::Started => self.started += 1,
            Phase::Finished(_) => self.finished += 1,
            _ => {}
        }
    }
}
struct UserApproval(bool);
impl<'a> Gate<conversation::Event<'a>> for UserApproval {
    fn before(&mut self, proposal: Proposal<conversation::Event<'a>>) -> Decision {
        if proposal.event.action == conversation::Action::AcceptUser && !self.0 {
            Decision::Pause
        } else {
            Decision::Allow
        }
    }
}
struct ModelPolicy;
impl<'a> Gate<model::Event<'a>> for ModelPolicy {
    fn before(&mut self, proposal: Proposal<model::Event<'a>>) -> Decision {
        let config = proposal.event.proposed;
        // Application-owned policy for this demonstration, not provider capability discovery.
        if matches!(config.model, "model-a" | "model-b")
            && matches!(
                config.reasoning,
                model::Reasoning::Default | model::Reasoning::Level("high")
            )
        {
            Decision::Allow
        } else {
            Decision::Deny
        }
    }
}
#[derive(Default)]
struct ReplyAudit {
    starts: usize,
    first_visible: usize,
    endings: usize,
    finalized: usize,
}
impl<'a> Observer<response::Event<'a>> for ReplyAudit {
    fn observe(&mut self, notice: Notice<response::Event<'a>>) {
        assert!(notice.proposal.event.payload.as_text().is_none());
        if notice.phase != Phase::Finished(Outcome::Succeeded) {
            return;
        }
        let e = notice.proposal.event;
        if e.action == response::Action::TurnStart {
            self.starts += 1;
        }
        if e.first_visible {
            self.first_visible += 1;
        }
        if e.action == response::Action::TurnEnd {
            self.endings += 1;
        }
        if e.action == response::Action::FinalizeAssistant {
            self.finalized += 1;
        }
    }
}
struct MemorySend<'a> {
    output: &'a mut [u8],
    used: usize,
}
impl Sender for MemorySend<'_> {
    fn send(&mut self, data: &[u8]) -> Result<SendReceipt, Fault> {
        self.output
            .get_mut(..data.len())
            .ok_or(Error::Capacity { needed: data.len() })?
            .copy_from_slice(data);
        self.used = data.len();
        Ok(SendReceipt {
            accepted_bytes: data.len(),
        })
    }
}
fn applied<T>(result: HookResult<T>) -> Result<T, Fault> {
    match result {
        HookResult::Applied(value) => Ok(value),
        HookResult::Paused => Err(Error::InvalidState.into()),
    }
}
pub fn main() -> Result<(), Fault> {
    let ids = Correlation {
        conversation: 1,
        run: 1,
        request: Some(1),
        message: Some(1),
        ..Correlation::default()
    };
    let user_text = "Review this change.";
    let blocks = [Content::text(user_text)];
    let messages = [Message::new(Role::User, &blocks)];
    let mut slots = [None; 4];
    let mut context = Context::new(
        &mut slots,
        Budget {
            limit: 4096,
            reserved: 1024,
        },
    )?;
    let mut chat = Conversation::new(ids);
    let mut conversation_hooks = Lifecycle::new(1, UserApproval(false), MetadataAudit::default());
    applied(chat.start(&mut conversation_hooks, || Ok(()))?)?;
    assert!(matches!(
        chat.user_message(1, Payload::utf8(user_text), &mut conversation_hooks, |_| {
            Err::<(), _>(Fault::Application(99))
        })?,
        HookResult::Paused
    ));
    conversation_hooks.gate_mut().0 = true;
    applied(
        chat.user_message(1, Payload::utf8(user_text), &mut conversation_hooks, |_| {
            // This callback is the actual history acceptance boundary.
            context.push(Group::new(1, &messages, 16), Overflow::Reject)?;
            Ok(())
        })?,
    )?;

    let mut model_hooks = Lifecycle::new(2, ModelPolicy, MetadataAudit::default());
    let mut settings = applied(ModelSettings::select(
        ids,
        model::Configuration::new("model-a"),
        &mut model_hooks,
    )?)?;
    applied(settings.switch_model("model-b", &mut model_hooks)?)?;
    applied(settings.set_reasoning(model::Reasoning::Level("high"), &mut model_hooks)?)?;
    assert_eq!(
        settings.current().reasoning,
        model::Reasoning::Level("high")
    );
    // Settings record intent; the application/zcall encoder still maps and validates
    // provider-specific reasoning options. This request demo uses default reasoning.
    applied(settings.set_reasoning(model::Reasoning::Default, &mut model_hooks)?)?;

    let mut request_hooks = Lifecycle::new(3, NoGate, MetadataAudit::default());
    let mut records = RecordStorage::<16>::new();
    let mut request = applied(context.build_with_lifecycle(
        ids,
        Protocol::OpenAiChat.into(),
        Presence::Value(Text::new(settings.current().model)),
        &mut records,
        &[],
        &mut request_hooks,
    )?)?;
    let mut wire = [0; 2048];
    let mut scratch = [0; 1024];
    let mut tokens = TokenStorage::<128>::new();
    let n =
        applied(
            request_hooks.run(ids, request::Event::new(request::Action::Encode), |_| {
                request
                    .encode(
                        Protocol::OpenAiChat.into(),
                        &mut wire,
                        &mut scratch,
                        &mut tokens,
                        EncodeContext::default(),
                        Policy::default(),
                    )
                    .map(|r| r.written)
                    .map_err(|e| Fault::Core(Error::Zcall(e)))
            })?,
        )?;
    let mut sent = [0; 2048];
    let mut sender = MemorySend {
        output: &mut sent,
        used: 0,
    };
    applied(send_with_hooks(
        ids,
        &wire[..n],
        &mut sender,
        &mut request_hooks,
    )?)?;
    assert_eq!(&sender.output[..sender.used], &wire[..n]);

    let mut frame = [0; 1024];
    let mut payload = [0; 1024];
    let mut aggregate = [0; 1024];
    let mut workspace = StreamWorkspace::<128, 32, 8>::new();
    let mut session = Session::new(
        SessionConfig::new(Protocol::OpenAiChat.into()),
        &mut frame,
        &mut payload,
        &mut aggregate,
        &mut workspace,
    )
    .map_err(Error::from)?;
    session.prepare().map_err(Error::from)?;
    let mut stream = ResponseStream::new(ids);
    let mut reply_hooks = Lifecycle::new(4, NoGate, ReplyAudit::default());
    let input = include_bytes!("../tests/fixtures/final.sse");
    let mut offset = 0;
    let mut answer = [0; 256];
    let mut used = 0;
    loop {
        let end = (offset + 13).min(input.len());
        let mut failure = None;
        let step = session.step(
            &input[offset..end],
            end == input.len(),
            |event| match stream.accept(event.data(), &mut reply_hooks, |data| {
                if let EventData::Delta {
                    kind: BlockType::Text,
                    text,
                    ..
                } = data
                {
                    let n = text.decode_into(&mut answer[used..]).map_err(Error::from)?;
                    used += n;
                }
                Ok(Control::Continue)
            }) {
                Ok(control) => control,
                Err(error) => {
                    failure = Some(error);
                    Control::Pause
                }
            },
        );
        if let Some(error) = failure {
            return Err(error);
        }
        offset += step.consumed;
        match step.status {
            StepStatus::Done => break,
            StepStatus::NeedInput | StepStatus::NeedEvents
                if step.consumed != 0 || step.emitted != 0 => {}
            _ => return Err(Error::InvalidState.into()),
        }
    }
    let answer = core::str::from_utf8(&answer[..used]).map_err(|_| Error::InvalidOutput)?;
    let mut saved = [0; 256];
    let mut saved_len = 0;
    let mut storage_hooks = Lifecycle::new(5, NoGate, MetadataAudit::default());
    applied(
        stream.finalize(Payload::utf8(answer), &mut reply_hooks, || {
            applied(
                storage_hooks.run(
                    ids,
                    persistence::Event::new(persistence::Action::SaveConversation)
                        .with_payload(Payload::utf8(answer)),
                    |_| {
                        saved[..answer.len()].copy_from_slice(answer.as_bytes());
                        saved_len = answer.len();
                        Ok(())
                    },
                )?,
            )
        })?,
    )?;
    assert_eq!(&saved[..saved_len], b"done");
    assert_eq!(reply_hooks.observer().starts, 1);
    assert_eq!(reply_hooks.observer().first_visible, 1);
    assert_eq!(reply_hooks.observer().endings, 1);
    assert_eq!(reply_hooks.observer().finalized, 1);
    applied(chat.close(&mut conversation_hooks, || Ok(()))?)?;
    println!(
        "Accepted user input, switched settings, encoded/sent a request, consumed and persisted an assistant reply through typed hooks."
    );
    let _unused = Lifecycle::new(6, NoGate, NoObserver);
    Ok(())
}
