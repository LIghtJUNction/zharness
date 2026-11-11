use std::cell::Cell;
use zharness::{Error, conversation::{Conversation, State as ConversationState}, hooks::{Decision, HookResult}, lifecycle::{Correlation, Event, Fault, Gate, Interceptor, Lifecycle, NoGate, NoObserver, Notice, Observer, Outcome, Payload, Phase, Proposal, domains::{model, request, response}}, request::{SendReceipt, Sender, send_with_hooks}, response::{ResponseStream, State}, settings::ModelSettings, zcall::v1::*};
const IDS: Correlation = Correlation { conversation: 1, run: 2, request: Some(3), message: Some(4), agent: None, attempt: 0 };
fn forbidden() -> Result<(), Fault> { panic!("blocked operation must not run") }
struct GateAll(Decision);
impl<E: Event> Gate<E> for GateAll { fn before(&mut self, _: Proposal<E>) -> Decision { self.0 } }
#[derive(Default)]
struct Log { phases: Vec<Phase>, first: usize, deltas: usize, terminal: Option<TurnOutcome> }
impl<'a> Observer<response::Event<'a>> for Log {
    fn observe(&mut self, n: Notice<response::Event<'a>>) {
        assert!(n.proposal.event.payload.as_text().is_none()); self.phases.push(n.phase);
        if n.phase == Phase::Finished(Outcome::Succeeded) {
            if n.proposal.event.first_visible { self.first += 1; }
            if n.proposal.event.action == response::Action::TextDelta { self.deltas += 1; }
            if n.proposal.event.terminal.is_some() { self.terminal = n.proposal.event.terminal; }
        }
    }
}
#[test]
fn conversation_accept_edit_close_commit_only_after_success() {
    let mut chat = Conversation::new(IDS); let mut bus = Lifecycle::new(1, GateAll(Decision::Pause), NoObserver);
    assert!(chat.user_message(1, Payload::utf8("x"), &mut bus, |_| Ok(())).is_err());
    assert!(matches!(chat.start(&mut bus, forbidden).unwrap(), HookResult::Paused)); assert_eq!(chat.state(), ConversationState::New);
    bus.gate_mut().0 = Decision::Allow; let _ = chat.start(&mut bus, || Ok(())).unwrap();
    assert_eq!(chat.user_message(1, Payload::utf8("x"), &mut bus, |_| Err::<(), _>(Fault::Application(9))), Err(Fault::Application(9)));
    assert_eq!(chat.revision(), 0);
    let _ = chat.user_message(1, Payload::utf8("x"), &mut bus, |p| { assert!(p.as_text().unwrap().eq_str("x")); Ok(()) }).unwrap();
    let _ = chat.edit_message(1, Payload::utf8("y"), &mut bus, |_| Ok(())).unwrap();
    let _ = chat.delete_message(1, &mut bus, || Ok(())).unwrap(); assert_eq!(chat.revision(), 3);
    bus.gate_mut().0 = Decision::Deny; assert_eq!(chat.close(&mut bus, forbidden), Err(Fault::Core(Error::Denied)));
    assert_eq!(chat.state(), ConversationState::Open);
    bus.gate_mut().0 = Decision::Allow; let _ = chat.close(&mut bus, || Ok(())).unwrap();
    assert!(chat.user_message(2, Payload::utf8("late"), &mut bus, |_| Ok(())).is_err());
}
#[test]
fn model_reasoning_and_intercepted_settings_do_not_change_on_pause_or_error() {
    let mut bus = Lifecycle::new(1, GateAll(Decision::Allow), NoObserver);
    let HookResult::Applied(mut settings) = ModelSettings::select(IDS, model::Configuration::new("a"), &mut bus).unwrap() else { panic!() };
    bus.gate_mut().0 = Decision::Pause;
    assert_eq!(settings.switch_model("b", &mut bus).unwrap(), HookResult::Paused);
    assert_eq!(settings.current().model, "a"); assert_eq!(settings.revision(), 0);
    bus.gate_mut().0 = Decision::Allow; let _ = settings.switch_model("b", &mut bus).unwrap();
    let _ = settings.set_reasoning(model::Reasoning::Level("high"), &mut bus).unwrap();
    assert_eq!(settings.current().reasoning, model::Reasoning::Level("high"));
    assert!(settings.set_reasoning(model::Reasoning::Level(""), &mut bus).is_err()); assert_eq!(settings.revision(), 2);
    struct Rename;
    impl<'a> Interceptor<model::Configuration<'a>> for Rename {
        fn intercept(&mut self, c: &mut model::Configuration<'a>) -> Result<(), Fault> { c.model = "approved"; Ok(()) }
    }
    let _ = settings.configure(model::Configuration::new("requested"), &mut Rename, &mut bus).unwrap();
    assert_eq!(settings.current().model, "approved"); assert_eq!(settings.revision(), 3);
}
#[test]
fn actual_send_is_gated_and_partial_acceptance_is_a_failure_without_retry() {
    struct Queue { calls: usize, partial: bool }
    impl Sender for Queue {
        fn send(&mut self, message: &[u8]) -> Result<SendReceipt, Fault> { self.calls += 1; Ok(SendReceipt { accepted_bytes: message.len() - usize::from(self.partial) }) }
    }
    struct SendObserver(Vec<Phase>);
    impl<'a> Observer<request::Event<'a>> for SendObserver { fn observe(&mut self, n: Notice<request::Event<'a>>) { assert!(n.proposal.event.payload.as_bytes().is_none()); self.0.push(n.phase); } }
    let mut bus = Lifecycle::new(1, GateAll(Decision::Deny), SendObserver(Vec::new())); let mut queue = Queue { calls: 0, partial: true };
    assert!(send_with_hooks(IDS, b"request", &mut queue, &mut bus).is_err()); assert_eq!(queue.calls, 0);
    bus.gate_mut().0 = Decision::Allow;
    assert_eq!(send_with_hooks(IDS, b"request", &mut queue, &mut bus), Err(Fault::Core(Error::InvalidOutput))); assert_eq!(queue.calls, 1);
    queue.partial = false;
    assert_eq!(send_with_hooks(IDS, b"request", &mut queue, &mut bus).unwrap(), HookResult::Applied(SendReceipt { accepted_bytes: 7 })); assert_eq!(queue.calls, 2);
}
#[test]
fn stream_gate_and_consumer_pause_do_not_accept_or_double_count_events() {
    let mut stream = ResponseStream::new(IDS); let mut bus = Lifecycle::new(1, GateAll(Decision::Pause), Log::default()); let calls = Cell::new(0);
    let started = EventData::Started { response_id: None, model: None, created: None };
    assert_eq!(stream.accept(started, &mut bus, |_| { calls.set(1); Ok(Control::Continue) }).unwrap(), Control::Pause);
    assert_eq!(calls.get(), 0); assert_eq!(stream.state(), State::New);
    bus.gate_mut().0 = Decision::Allow;
    assert_eq!(stream.accept(started, &mut bus, |_| Ok(Control::Pause)).unwrap(), Control::Pause); assert_eq!(stream.accepted_events(), 0);
    stream.accept(started, &mut bus, |_| Ok(Control::Continue)).unwrap();
    let delta = EventData::Delta { key: EventKey::default(), kind: BlockType::Text, text: Text::new("hello") };
    assert_eq!(stream.accept(delta, &mut bus, |_| Ok(Control::Pause)).unwrap(), Control::Pause); assert!(!stream.has_visible_content());
    stream.accept(delta, &mut bus, |_| Ok(Control::Continue)).unwrap(); stream.accept(delta, &mut bus, |_| Ok(Control::Continue)).unwrap();
    assert_eq!(bus.observer().first, 1); assert_eq!(bus.observer().deltas, 2); assert_eq!(stream.accepted_events(), 3);
}
#[test]
fn real_zcall_response_drives_hooks_and_explicit_assistant_finalization() {
    let input = include_bytes!("fixtures/final.sse");
    let mut frame = [0; 1024]; let mut payload = [0; 1024]; let mut aggregate = [0; 1024]; let mut workspace = StreamWorkspace::<128, 32, 8>::new();
    let mut session = Session::new(SessionConfig::new(Protocol::OpenAiChat.into()), &mut frame, &mut payload, &mut aggregate, &mut workspace).unwrap(); session.prepare().unwrap();
    let mut stream = ResponseStream::new(IDS); let mut bus = Lifecycle::new(1, NoGate, Log::default()); let mut text = String::new(); let mut offset = 0;
    loop {
        let end = (offset + 11).min(input.len());
        let step = session.step(&input[offset..end], end == input.len(), |event| stream.accept(event.data(), &mut bus, |data| {
            if let EventData::Delta { kind: BlockType::Text, text: delta, .. } = data {
                let mut decoded = [0; 64]; let n = delta.decode_into(&mut decoded).unwrap(); text.push_str(core::str::from_utf8(&decoded[..n]).unwrap());
            }
            Ok(Control::Continue)
        }).unwrap());
        offset += step.consumed; if step.status == StepStatus::Done { break; } assert!(step.consumed != 0 || step.emitted != 0);
    }
    assert_eq!(text, "done"); assert_eq!(bus.observer().first, 1); assert_eq!(stream.state(), State::Ended(TurnOutcome::Completed));
    let committed = Cell::new(false);
    let _ = stream.finalize(Payload::utf8(&text), &mut bus, || { committed.set(true); Ok(()) }).unwrap(); assert!(committed.get());
    assert!(stream.finalize(Payload::utf8(&text), &mut bus, || Ok(())).is_err());
}
#[test]
fn waiting_tools_is_not_a_final_assistant_answer() {
    let mut stream = ResponseStream::new(IDS); let mut bus = Lifecycle::new(1, NoGate, NoObserver);
    let terminal = Terminal { outcome: TurnOutcome::WaitingTools, usage: StreamUsage::default(), error: None, upstream_code: None, upstream_message: None };
    stream.accept(EventData::Terminal(terminal), &mut bus, |_| Ok(Control::Continue)).unwrap();
    assert!(stream.finalize(Payload::metadata(0), &mut bus, forbidden).is_err());
    assert!(stream.accept(EventData::Terminal(terminal), &mut bus, |_| Ok(Control::Continue)).is_err());
}
