use super::measure;
use std::hint::black_box;
use zharness::{
    ToolLoop,
    cluster::{Agent, Assignment, Envelope, Mailbox, Task, TaskBoard, TaskResult, TaskSlot},
    context::{Budget, Context, Group, Message, Overflow},
    hooks::{After, Before, Decision, Hooks, NoHooks},
    turn::{CallSlot, LoopLimits},
    zcall::v1::{Content, Role},
};

static TEXT: [Content<'static>; 1] = [Content::text("benchmark")];
static MESSAGES: [Message<'static>; 1] = [Message::new(Role::User, &TEXT)];
struct Audit(u64);
impl Hooks for Audit {
    fn before(&mut self, _: Before<'_>) -> Decision {
        self.0 += 1;
        Decision::Allow
    }
    fn after(&mut self, _: After<'_>) {
        self.0 += 1;
    }
}
struct Guard(u64);
impl Hooks for Guard {
    fn before(&mut self, e: Before<'_>) -> Decision {
        match e {
            Before::Model { run, .. } if run != self.0 => Decision::Deny,
            _ => Decision::Allow,
        }
    }
}
pub(super) fn run() {
    for count in [1, 4, 16, 64, 256, 1024] {
        let mut storage = vec![None; count];
        let mut fork_storage = vec![None; count];
        let mut context = Context::new(
            &mut storage,
            Budget {
                limit: count as u64,
                reserved: 0,
            },
        )
        .unwrap();
        for id in 0..count as u64 {
            context
                .push(Group::new(id, &MESSAGES, 1), Overflow::Reject)
                .unwrap();
        }
        let mut id = count as u64;
        measure("context_slide", count, context.memory_bytes(), || {
            black_box(
                context
                    .push(
                        Group::new(black_box(id), &MESSAGES, 1),
                        Overflow::EvictOldest,
                    )
                    .unwrap(),
            );
            id += 1;
        });
        measure(
            "context_fork",
            count,
            context.memory_bytes() + std::mem::size_of_val(fork_storage.as_slice()),
            || {
                let fork = context.fork_into(&mut fork_storage).unwrap();
                black_box(&fork);
                black_box(fork.tokens());
            },
        );
        let agents = [Agent {
            id: 1,
            capabilities: 1,
        }];
        let mut tasks = vec![TaskSlot::EMPTY; count];
        let mut board = TaskBoard::new(1, &agents, &mut tasks).unwrap();
        for id in 0..count as u64 {
            board
                .submit(Task {
                    id,
                    requirements: if id + 1 == count as u64 { 1 } else { 2 },
                    payload: b"work",
                    after: &[],
                })
                .unwrap();
        }
        measure(
            "task_claim_complete_requeue",
            count,
            board.memory_bytes(),
            || {
                let Assignment::Lease(lease) =
                    board.claim(black_box(1), 0, 100, &mut NoHooks).unwrap()
                else {
                    panic!("eligible task missing")
                };
                board
                    .complete(
                        lease,
                        0,
                        TaskResult {
                            output: black_box(b"done"),
                            is_error: false,
                        },
                        &mut NoHooks,
                    )
                    .unwrap();
                board.retire(lease.task()).unwrap();
                board
                    .submit(Task {
                        id: lease.task(),
                        requirements: 1,
                        payload: b"work",
                        after: &[],
                    })
                    .unwrap();
                black_box(&board);
            },
        );
        let mut slots = vec![None; count];
        let mut mailbox = Mailbox::new(&mut slots);
        for id in 0..count as u64 {
            mailbox
                .send(Envelope {
                    from: 1,
                    to: 2,
                    correlation: id,
                    payload: b"message",
                })
                .unwrap();
        }
        measure(
            "mailbox_receive_send",
            count,
            mailbox.memory_bytes(),
            || {
                let message = mailbox.receive_for(black_box(2)).unwrap();
                mailbox.send(black_box(message)).unwrap();
            },
        );
    }
    let mut hooks = (Audit(0), Guard(7));
    measure("hook_chain_two", 2, std::mem::size_of_val(&hooks), || {
        assert_eq!(
            black_box(&mut hooks).before(black_box(Before::Model { run: 7, round: 1 })),
            Decision::Allow
        );
        hooks.after(black_box(After::Stopped { run: 7 }));
        black_box(&hooks);
    });
    let mut slots = [const { CallSlot::<256>::new() }; 4];
    let memory = ToolLoop::new(1, &mut slots, LoopLimits::default()).memory_bytes();
    measure("loop_start_cancel", 4, memory, || {
        let mut flow = ToolLoop::new(black_box(1), &mut slots, LoopLimits::default());
        let _ = black_box(black_box(&mut flow).next(&mut NoHooks).unwrap());
        assert!(black_box(&mut flow).cancel(&mut NoHooks));
        black_box(&flow);
    });
}
