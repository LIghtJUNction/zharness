use zharness::{
    Error,
    cluster::{
        Agent, Assignment, Envelope, Join, Lease, Mailbox, Task, TaskBoard, TaskResult, TaskSlot,
        TaskState,
    },
    hooks::{After, Before, Decision, HookResult, Hooks, NoHooks},
};

const AGENTS: [Agent; 3] = [
    Agent {
        id: 1,
        capabilities: 1,
    },
    Agent {
        id: 2,
        capabilities: 3,
    },
    Agent {
        id: 3,
        capabilities: 4,
    },
];
fn task(id: u64, requirements: u64, after: &[u64]) -> Task<'_> {
    Task {
        id,
        requirements,
        payload: b"work",
        after,
    }
}
fn leased(assignment: Assignment) -> Lease {
    let Assignment::Lease(lease) = assignment else {
        panic!("expected lease, got {assignment:?}")
    };
    lease
}
const OK: TaskResult<'static> = TaskResult {
    output: b"ok",
    is_error: false,
};

#[test]
fn fanout_join_capabilities_and_bounded_retirement() {
    let mut slots = [TaskSlot::EMPTY; 3];
    let mut board = TaskBoard::new(1, &AGENTS, &mut slots).unwrap();
    board.submit(task(10, 1, &[])).unwrap();
    board.submit(task(20, 2, &[])).unwrap();
    board.submit(task(30, 4, &[10, 20])).unwrap();
    let a = leased(board.claim(1, 0, 10, &mut NoHooks).unwrap());
    let b = leased(board.claim(2, 0, 10, &mut NoHooks).unwrap());
    assert_eq!(a.task(), 10);
    assert_eq!(b.task(), 20);
    assert_eq!(
        board.claim(3, 0, 10, &mut NoHooks).unwrap(),
        Assignment::Idle
    );
    assert_eq!(board.claim(1, 0, 10, &mut NoHooks), Err(Error::Busy));
    board.complete(b, 1, OK, &mut NoHooks).unwrap();
    board.complete(a, 2, OK, &mut NoHooks).unwrap();
    assert_eq!(board.join(&[10, 20]).unwrap(), Join::Ready);
    let joined = leased(board.claim(3, 3, 10, &mut NoHooks).unwrap());
    assert_eq!(joined.task(), 30);
    assert_eq!(board.task(joined, 3).unwrap().payload, b"work");
    board.complete(joined, 4, OK, &mut NoHooks).unwrap();
    assert_eq!(board.join(&[10, 20, 30]).unwrap(), Join::Ready);
    assert_eq!(board.retire(10), Err(Error::Busy));
    board.retire(30).unwrap();
    board.retire(10).unwrap();
    board.retire(20).unwrap();
    assert!(board.is_empty());
    board.submit(task(10, 1, &[])).unwrap();
    let new = leased(board.claim(1, 5, 10, &mut NoHooks).unwrap());
    assert!(new.attempt() > a.attempt());
    assert_eq!(board.complete(a, 5, OK, &mut NoHooks), Err(Error::Stale));
}
#[test]
fn handoff_renew_expire_and_late_results_are_fenced() {
    let mut slots = [TaskSlot::EMPTY; 1];
    let mut board = TaskBoard::new(7, &AGENTS, &mut slots).unwrap();
    board.submit(task(1, 1, &[])).unwrap();
    let old = leased(board.claim(1, 0, 5, &mut NoHooks).unwrap());
    assert_eq!(
        board.handoff(old, 3, 1, 5, &mut NoHooks),
        Err(Error::Incompatible)
    );
    let HookResult::Applied(next) = board.handoff(old, 2, 1, 5, &mut NoHooks).unwrap() else {
        panic!()
    };
    assert_eq!(board.complete(old, 2, OK, &mut NoHooks), Err(Error::Stale));
    let renewed = board.renew(next, 2, 10).unwrap();
    assert_eq!(board.complete(next, 3, OK, &mut NoHooks), Err(Error::Stale));
    assert_eq!(board.renew(renewed, 1, 20), Err(Error::InvalidState));
    assert_eq!(
        board.complete(renewed, 12, OK, &mut NoHooks),
        Err(Error::Stale)
    );
    assert_eq!(board.expire(12, &mut NoHooks).unwrap(), 1);
    let final_lease = leased(board.claim(1, 12, 10, &mut NoHooks).unwrap());
    assert!(final_lease.attempt() > renewed.attempt());
    board.cancel(1, &mut NoHooks).unwrap();
    assert_eq!(
        board.complete(final_lease, 13, OK, &mut NoHooks),
        Err(Error::Stale)
    );
}
#[test]
fn failure_propagates_through_dependencies_without_recursion() {
    let mut slots = [TaskSlot::EMPTY; 4];
    let mut board = TaskBoard::new(1, &AGENTS, &mut slots).unwrap();
    board.submit(task(1, 1, &[])).unwrap();
    board.submit(task(2, 1, &[1])).unwrap();
    board.submit(task(3, 1, &[2])).unwrap();
    let lease = leased(board.claim(1, 0, 10, &mut NoHooks).unwrap());
    board
        .complete(
            lease,
            1,
            TaskResult {
                output: b"failed",
                is_error: true,
            },
            &mut NoHooks,
        )
        .unwrap();
    assert!(matches!(board.state(2).unwrap(), TaskState::Blocked { .. }));
    assert!(matches!(board.state(3).unwrap(), TaskState::Blocked { .. }));
    assert_eq!(
        board.claim(2, 2, 10, &mut NoHooks).unwrap(),
        Assignment::Idle
    );
    assert_eq!(board.join(&[3]).unwrap(), Join::Failed(3));
    assert!(board.complete(lease, 2, OK, &mut NoHooks).is_err());
}
#[test]
fn rejects_cycles_duplicates_capacity_bad_clocks_and_deadline_overflow() {
    let mut slots = [TaskSlot::EMPTY; 1];
    assert!(TaskBoard::new(1, &[AGENTS[0], AGENTS[0]], &mut slots).is_err());
    let mut board = TaskBoard::new(1, &AGENTS, &mut slots).unwrap();
    assert_eq!(board.submit(task(1, 1, &[1])), Err(Error::InvalidState));
    assert_eq!(board.submit(task(1, 1, &[2])), Err(Error::UnknownId));
    board.submit(task(1, 1, &[])).unwrap();
    assert_eq!(board.submit(task(1, 1, &[])), Err(Error::DuplicateId));
    assert_eq!(board.submit(task(2, 1, &[1, 1])), Err(Error::DuplicateId));
    assert_eq!(
        board.submit(task(2, 1, &[])),
        Err(Error::Capacity { needed: 2 })
    );
    assert_eq!(board.claim(999, 0, 10, &mut NoHooks), Err(Error::UnknownId));
    assert_eq!(
        board.claim(1, u64::MAX, 1, &mut NoHooks),
        Err(Error::Overflow)
    );
    assert_eq!(board.claim(1, 0, 0, &mut NoHooks), Err(Error::InvalidState));
    assert_eq!(board.state(1).unwrap(), TaskState::Pending);
    let lease = leased(board.claim(1, 5, 10, &mut NoHooks).unwrap());
    assert_eq!(board.expire(4, &mut NoHooks), Err(Error::InvalidState));
    assert_eq!(board.renew(lease, 6, u64::MAX), Err(Error::Overflow));
    assert_eq!(board.state(1).unwrap(), TaskState::Running(lease));
}
#[test]
fn paused_assignment_does_not_consume_attempt_or_capacity() {
    struct Gate {
        pause: bool,
        events: usize,
    }
    impl Hooks for Gate {
        fn before(&mut self, _: Before<'_>) -> Decision {
            if self.pause {
                Decision::Pause
            } else {
                Decision::Allow
            }
        }
        fn after(&mut self, _: After<'_>) {
            self.events += 1;
        }
    }
    let mut slots = [TaskSlot::EMPTY; 1];
    let mut board = TaskBoard::new(1, &AGENTS, &mut slots).unwrap();
    board.submit(task(1, 1, &[])).unwrap();
    let mut hooks = Gate {
        pause: true,
        events: 0,
    };
    assert_eq!(
        board.claim(1, 0, 10, &mut hooks).unwrap(),
        Assignment::Paused
    );
    assert_eq!(hooks.events, 0);
    assert_eq!(board.state(1).unwrap(), TaskState::Pending);
    hooks.pause = false;
    let lease = leased(board.claim(1, 0, 10, &mut hooks).unwrap());
    assert_eq!(lease.attempt(), 1);
    assert_eq!(hooks.events, 1);
}
#[test]
fn mailbox_backpressure_returns_ownership_and_preserves_order() {
    let mut slots = [None; 2];
    let mut mail = Mailbox::new(&mut slots);
    let a = Envelope {
        from: 1,
        to: 2,
        correlation: 10,
        payload: b"a",
    };
    let b = Envelope {
        from: 1,
        to: 3,
        correlation: 11,
        payload: b"b",
    };
    mail.send(a).unwrap();
    mail.send(b).unwrap();
    assert_eq!(mail.send(a), Err(a));
    assert_eq!(mail.receive_for(3), Some(b));
    assert_eq!(mail.receive_for(3), None);
    assert_eq!(mail.receive(), Some(a));
    assert!(mail.is_empty());
    let mut owned_slots = [None];
    let mut owned = Mailbox::new(&mut owned_slots);
    owned.send(String::from("first")).unwrap();
    assert_eq!(owned.send(String::from("retry")).unwrap_err(), "retry");
    assert_eq!(owned.receive().unwrap(), "first");
}
