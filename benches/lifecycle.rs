//! Local typed-hook costs only. No provider, network, persistence or UI timings.
use std::{alloc::{GlobalAlloc, Layout, System}, hint::black_box, sync::atomic::{AtomicUsize, Ordering}, time::Instant};
use zharness::{conversation::Conversation, hooks::{Decision, HookResult}, lifecycle::{Correlation, Event, Fault, Gate, Lifecycle, NoGate, NoObserver, Notice, Observer, Outcome, Payload, Proposal, domains::{model, prompt, request}}, request::{Sender, SendReceipt, send_with_hooks}, response::ResponseStream, settings::ModelSettings, zcall::v1::*};
struct Allocations;
static COUNT: AtomicUsize = AtomicUsize::new(0);
// SAFETY: all pointer/layout/size arguments are forwarded unchanged to System.
// Benchmark instrumentation is not linked into the unsafe-forbidden core.
unsafe impl GlobalAlloc for Allocations {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 { COUNT.fetch_add(1, Ordering::Relaxed); unsafe { System.alloc(l) } }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 { COUNT.fetch_add(1, Ordering::Relaxed); unsafe { System.alloc_zeroed(l) } }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) { unsafe { System.dealloc(p, l) } }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 { COUNT.fetch_add(1, Ordering::Relaxed); unsafe { System.realloc(p, l, n) } }
}
#[global_allocator]
static ALLOCATOR: Allocations = Allocations;
const IDS: Correlation = Correlation { conversation: 1, run: 1, request: Some(1), message: Some(1), agent: None, attempt: 0 };
fn measure(name: &str, bytes: usize, memory: usize, mut op: impl FnMut()) {
    let smoke = std::env::var_os("ZHARNESS_BENCH_SMOKE").is_some() || std::env::args().any(|a| a == "--test");
    let samples = if smoke { 31 } else { 1001 }; let batch = if smoke { 32 } else { 4096 };
    let mut timings = Vec::with_capacity(samples);
    for _ in 0..32 { op(); }
    let before = COUNT.load(Ordering::Relaxed); let start = Instant::now();
    for _ in 0..batch { op(); }
    let mean = start.elapsed().as_nanos() as f64 / batch as f64;
    for _ in 0..samples { let start = Instant::now(); op(); timings.push(start.elapsed().as_nanos()); }
    let allocations = COUNT.load(Ordering::Relaxed) - before;
    assert_eq!(allocations, 0, "allocation in {name}"); timings.sort_unstable();
    let p = |n: usize| timings[(timings.len() - 1) * n / 100];
    println!("{name},{bytes},{mean:.2},{},{},{},{allocations},{memory}", p(50), p(95), p(99));
}
#[derive(Default)]
struct Audit(u64);
impl<E: Event> Observer<E> for Audit { fn observe(&mut self, n: Notice<E>) { black_box(n); self.0 += 1; } }
struct Allow;
impl<E: Event> Gate<E> for Allow { fn before(&mut self, p: Proposal<E>) -> Decision { black_box(p.correlation); Decision::Allow } }
struct Memory<'a>(&'a mut [u8]);
impl Sender for Memory<'_> {
    fn send(&mut self, bytes: &[u8]) -> Result<SendReceipt, Fault> { self.0[..bytes.len()].copy_from_slice(bytes); Ok(SendReceipt { accepted_bytes: bytes.len() }) }
}
fn main() {
    println!("case,input_bytes,mean_ns,p50_instrumented_ns,p95_instrumented_ns,p99_instrumented_ns,total_allocations,workspace_bytes");
    let mut bare = Lifecycle::new(1, NoGate, NoObserver);
    measure("lifecycle_no_hooks", 0, bare.memory_bytes(), || {
        let result = black_box(&mut bare).run(IDS, prompt::Event::new(prompt::Action::Build), |_| Ok(black_box(7u64))).unwrap();
        assert_eq!(black_box(result), HookResult::Applied(7));
    });
    let mut hooks = Lifecycle::new(2, (Allow, Allow), (Audit::default(), Audit::default()));
    measure("lifecycle_two_gates_observers", 0, hooks.memory_bytes(), || {
        let result = black_box(&mut hooks).run(IDS, prompt::Event::new(prompt::Action::Build), |_| Ok(black_box(7u64))).unwrap();
        assert_eq!(black_box(result), HookResult::Applied(7)); black_box(hooks.observer());
    });
    measure("lifecycle_split_finish", 0, hooks.memory_bytes(), || {
        let HookResult::Applied(mut span) = hooks.begin(IDS, request::Event::new(request::Action::Send)).unwrap() else { panic!() };
        black_box(&span); hooks.finish(&mut span, Outcome::Succeeded).unwrap(); let _ = black_box(span);
    });
    let HookResult::Applied(mut settings) = ModelSettings::select(IDS, model::Configuration::new("a"), &mut bare).unwrap() else { panic!() };
    let mut second = false;
    measure("model_switch_hooked", 0, std::mem::size_of_val(&settings) + bare.memory_bytes(), || {
        second = !second; let _ = settings.switch_model(black_box(if second { "b" } else { "a" }), &mut bare).unwrap(); black_box(settings.current());
    });
    for size in [64, 1024, 4096] {
        let input = "x".repeat(size); let mut storage = vec![0; size]; let mut sender = Memory(&mut storage);
        measure("send_memory_hooked", size, size + hooks.memory_bytes(), || {
            let receipt = send_with_hooks(IDS, black_box(input.as_bytes()), &mut sender, &mut hooks).unwrap();
            let _ = black_box(receipt); black_box(&sender.0);
        });
        measure("conversation_accept_hooked", size, std::mem::size_of::<Conversation>() + hooks.memory_bytes(), || {
            let mut chat = Conversation::new(IDS); let _ = chat.start(&mut hooks, || Ok(())).unwrap();
            let result = chat.user_message(1, Payload::utf8(black_box(&input)), &mut hooks, |p| Ok(black_box(p.len()))).unwrap();
            let _ = black_box(result); black_box(chat.revision());
        });
        measure("normalized_response_hooks", size, std::mem::size_of::<ResponseStream>() + hooks.memory_bytes(), || {
            let mut stream = ResponseStream::new(IDS);
            stream.accept(EventData::Started { response_id: None, model: None, created: None }, &mut hooks, |_| Ok(Control::Continue)).unwrap();
            stream.accept(EventData::Delta { key: EventKey::default(), kind: BlockType::Text, text: Text::new(black_box(&input)) }, &mut hooks, |data| { black_box(data); Ok(Control::Continue) }).unwrap();
            stream.accept(EventData::Terminal(Terminal { outcome: TurnOutcome::Completed, usage: StreamUsage::default(), error: None, upstream_code: None, upstream_message: None }), &mut hooks, |_| Ok(Control::Continue)).unwrap();
            black_box(&stream);
        });
    }
}
