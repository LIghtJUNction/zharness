//! Local coordination example. Workers may be local or remotely driven by an
//! application transport; this board itself neither starts nor connects workers.
use zharness::{
    Error,
    cluster::{Agent, Assignment, Envelope, Join, Mailbox, Task, TaskBoard, TaskResult, TaskSlot},
    hooks::NoHooks,
};

fn main() -> Result<(), Error> {
    let agents = [
        Agent {
            id: 1,
            capabilities: 1,
        },
        Agent {
            id: 2,
            capabilities: 1,
        },
        Agent {
            id: 3,
            capabilities: 2,
        },
    ];
    let mut summary = [0u8; 64];
    let mut task_slots = [TaskSlot::EMPTY; 4];
    let mut mail_slots = [None; 2];
    let mut board = TaskBoard::new(100, &agents, &mut task_slots)?;
    let mut mailbox = Mailbox::new(&mut mail_slots);
    board.submit(Task {
        id: 10,
        requirements: 1,
        payload: b"Review design",
        after: &[],
    })?;
    board.submit(Task {
        id: 20,
        requirements: 1,
        payload: b"Review tests",
        after: &[],
    })?;
    board.submit(Task {
        id: 30,
        requirements: 2,
        payload: b"Summarize both reviews",
        after: &[10, 20],
    })?;
    let Assignment::Lease(a) = board.claim(1, 0, 10, &mut NoHooks)? else {
        return Err(Error::InvalidState);
    };
    let Assignment::Lease(b) = board.claim(2, 0, 10, &mut NoHooks)? else {
        return Err(Error::InvalidState);
    };
    // Real applications run their own Context/ToolLoop for each claimed task.
    let result_a = TaskResult {
        output: b"design checked;",
        is_error: false,
    };
    let result_b = TaskResult {
        output: b"tests checked;",
        is_error: false,
    };
    board.complete(b, 1, result_b, &mut NoHooks)?;
    board.complete(a, 1, result_a, &mut NoHooks)?;
    for (lease, result) in [(b, result_b), (a, result_a)] {
        mailbox
            .send(Envelope {
                from: lease.agent(),
                to: 3,
                correlation: lease.task(),
                payload: result.output,
            })
            .map_err(|_| Error::Capacity { needed: 3 })?;
    }
    assert_eq!(board.join(&[10, 20])?, Join::Ready);
    let Assignment::Lease(joined) = board.claim(3, 2, 10, &mut NoHooks)? else {
        return Err(Error::InvalidState);
    };
    let mut used = 0;
    while let Some(message) = mailbox.receive_for(3) {
        let end = used + message.payload.len();
        summary
            .get_mut(used..end)
            .ok_or(Error::Capacity { needed: end })?
            .copy_from_slice(message.payload);
        used = end;
    }
    board.complete(
        joined,
        3,
        TaskResult {
            output: &summary[..used],
            is_error: false,
        },
        &mut NoHooks,
    )?;
    assert_eq!(board.join(&[10, 20, 30])?, Join::Ready);
    println!(
        "{}",
        core::str::from_utf8(&summary[..used]).expect("demo text is UTF-8")
    );
    Ok(())
}
