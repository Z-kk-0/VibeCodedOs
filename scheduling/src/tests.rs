use super::*;

/// Task data = a name, to see who runs.
fn queue() -> RunQueue<&'static str> {
    RunQueue::new("boot", "idle")
}

fn anyone(_: &&'static str) -> bool {
    true
}

fn run_order(queue: &mut RunQueue<&'static str>, switches: usize) -> Vec<&'static str> {
    (0..switches)
        .map(|_| {
            queue.switch(0, 0, anyone);
            *queue.current(0)
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
    queue.set_current_state(0, State::Sleeping { until_tick: 10 });
    queue.switch(0, 0, anyone);
    assert_eq!(*queue.current(0), "idle");
    queue.switch(0, 5, anyone);
    assert_eq!(*queue.current(0), "idle");
}

#[test]
fn sleepers_wake_up_on_their_tick() {
    let mut queue = queue();
    queue.spawn("a");
    queue.set_current_state(0, State::Sleeping { until_tick: 3 }); // boot sleeps
    assert_eq!(run_order(&mut queue, 2), ["a", "a"]);
    queue.switch(0, 3, anyone);
    assert_eq!(*queue.current(0), "boot");
}

#[test]
fn a_lone_ready_task_keeps_running() {
    let mut queue = queue();
    assert_eq!(run_order(&mut queue, 3), ["boot", "boot", "boot"]);
    assert!(!queue.switch(0, 0, anyone), "nothing changed");
}

#[test]
fn the_idle_task_cannot_sleep_or_exit() {
    let mut queue = queue();
    queue.set_current_state(0, State::Exited); // boot exits
    queue.switch(0, 0, anyone);
    assert_eq!(*queue.current(0), "idle");
    queue.set_current_state(0, State::Exited); // ignored for idle
    queue.switch(0, 0, anyone);
    assert_eq!(*queue.current(0), "idle");
}

#[test]
fn exited_tasks_are_handed_back_but_never_a_running_one() {
    let mut queue = queue();
    let a = queue.spawn("a");
    queue.switch(0, 0, anyone);
    assert_eq!(queue.current_id(0), a);
    queue.set_current_state(0, State::Exited);

    assert_eq!(queue.take_exited(anyone), None); // "a" still runs: its stack is in use

    queue.switch(0, 0, anyone);
    assert_eq!(queue.take_exited(|_| false), None); // the kernel says: not yet
    assert_eq!(queue.take_exited(anyone), Some("a"));
    assert_eq!(queue.task_count(), 2);
    assert_eq!(*queue.current(0), "boot");
}

#[test]
fn removing_a_task_keeps_the_current_one_current() {
    let mut queue = queue();
    queue.spawn("a");
    queue.spawn("b");
    queue.switch(0, 0, anyone); // a
    queue.set_current_state(0, State::Exited);
    queue.switch(0, 0, anyone); // b
    assert_eq!(*queue.current(0), "b");

    assert_eq!(queue.take_exited(anyone), Some("a"));

    assert_eq!(*queue.current(0), "b");
    assert_eq!(run_order(&mut queue, 2), ["boot", "b"]);
}

#[test]
fn a_new_core_starts_on_its_idle_task() {
    let mut queue = queue();
    assert_eq!(queue.add_core("idle 1"), 1);
    assert_eq!(queue.core_count(), 2);
    assert_eq!(*queue.current(1), "idle 1");
    queue.switch(1, 0, anyone);
    assert_eq!(*queue.current(1), "idle 1", "boot already runs on core 0");
}

#[test]
fn two_cores_never_run_the_same_task() {
    let mut queue = queue();
    queue.add_core("idle 1");
    queue.spawn("a");
    queue.spawn("b");
    for _ in 0..10 {
        for core in 0..2 {
            queue.switch(core, 0, anyone);
            assert_ne!(queue.current_id(0), queue.current_id(1));
        }
    }
}

#[test]
fn cores_share_the_ready_tasks() {
    let mut queue = queue();
    queue.add_core("idle 1");
    queue.spawn("a");
    queue.switch(1, 0, anyone);
    assert_eq!(*queue.current(1), "a");
    queue.switch(0, 0, anyone);
    assert_eq!(*queue.current(0), "boot", "a is taken, boot keeps core 0");
    queue.set_current_state(1, State::Exited);
    queue.switch(1, 0, anyone);
    assert_eq!(*queue.current(1), "idle 1");
}

#[test]
fn a_task_the_kernel_holds_back_is_skipped() {
    let mut queue = queue();
    queue.add_core("idle 1");
    queue.spawn("a");
    queue.switch(1, 0, |task| *task != "a");
    assert_eq!(*queue.current(1), "idle 1");
    queue.switch(1, 0, anyone);
    assert_eq!(*queue.current(1), "a");
}

#[test]
fn idle_tasks_stay_on_their_own_core() {
    let mut queue = queue();
    queue.add_core("idle 1");
    queue.set_current_state(0, State::Exited); // boot exits
    queue.switch(0, 0, anyone);
    assert_eq!(*queue.current(0), "idle");
    queue.switch(1, 0, anyone);
    assert_eq!(*queue.current(1), "idle 1");
}
