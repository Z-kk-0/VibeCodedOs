use super::*;

/// Task data = a name, to see who runs.
fn queue() -> RunQueue<&'static str> {
    RunQueue::new("boot", "idle")
}

fn run_order(queue: &mut RunQueue<&'static str>, switches: usize) -> Vec<&'static str> {
    (0..switches)
        .map(|_| {
            queue.switch(0);
            *queue.current()
        })
        .collect()
}

#[test]
fn ready_tasks_take_turns() {
    let mut queue = queue();
    queue.spawn("a");
    queue.spawn("b");
    assert_eq!(
        run_order(&mut queue, 6),
        ["a", "b", "boot", "a", "b", "boot"]
    );
}

#[test]
fn idle_runs_only_when_nobody_else_can() {
    let mut queue = queue();
    queue.set_current_state(State::Sleeping { until_tick: 10 });
    queue.switch(0);
    assert_eq!(*queue.current(), "idle");
    queue.switch(5);
    assert_eq!(*queue.current(), "idle");
}

#[test]
fn sleepers_wake_up_on_their_tick() {
    let mut queue = queue();
    queue.spawn("a");
    queue.set_current_state(State::Sleeping { until_tick: 3 }); // boot sleeps
    assert_eq!(run_order(&mut queue, 2), ["a", "a"]);
    queue.switch(3);
    assert_eq!(*queue.current(), "boot");
}

#[test]
fn a_lone_ready_task_keeps_running() {
    let mut queue = queue();
    assert_eq!(run_order(&mut queue, 3), ["boot", "boot", "boot"]);
}

#[test]
fn the_idle_task_cannot_sleep_or_exit() {
    let mut queue = queue();
    queue.set_current_state(State::Exited); // boot exits
    queue.switch(0);
    assert_eq!(*queue.current(), "idle");
    queue.set_current_state(State::Exited); // ignored for idle
    queue.switch(0);
    assert_eq!(*queue.current(), "idle");
}

#[test]
fn exited_tasks_are_handed_back_but_never_the_running_one() {
    let mut queue = queue();
    let a = queue.spawn("a");
    queue.switch(0);
    assert_eq!(queue.current_id(), a);
    queue.set_current_state(State::Exited);

    assert_eq!(queue.take_exited(), None); // "a" is still running – its stack is in use

    queue.switch(0);
    assert_eq!(queue.take_exited(), Some("a"));
    assert_eq!(queue.task_count(), 2);
    assert_eq!(*queue.current(), "boot");
}

#[test]
fn removing_a_task_keeps_the_current_one_current() {
    let mut queue = queue();
    queue.spawn("a");
    queue.spawn("b");
    queue.switch(0); // a
    queue.set_current_state(State::Exited);
    queue.switch(0); // b
    assert_eq!(*queue.current(), "b");

    assert_eq!(queue.take_exited(), Some("a"));

    assert_eq!(*queue.current(), "b");
    assert_eq!(run_order(&mut queue, 2), ["boot", "b"]);
}
