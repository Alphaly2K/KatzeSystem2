use std::collections::BTreeMap;

use katze_system2::{
    Error, Result,
    expression::State,
    kcs::{CoreHost, OrdinaryTask, Program},
    task::{Command, Control, Lifecycle, TaskClock, TaskTree},
};

fn ins(code: &mut Vec<u8>, opcode: u8, args: &[u32]) {
    code.push(opcode);
    for arg in args {
        code.push(3);
        code.extend(arg.to_le_bytes());
    }
}

fn task(id: u32) -> OrdinaryTask {
    let mut code = Vec::new();
    ins(&mut code, 0x25, &[32]);
    ins(&mut code, 0x2b, &[71, 4]);
    ins(&mut code, 0x26, &[0]);
    ins(&mut code, 0x2b, &[5, 0]);
    ins(&mut code, 0x26, &[4]);
    ins(&mut code, 0x29, &[0]);
    let mut task =
        OrdinaryTask::new(Program::from_code(&code, vec![0; 64], 64).unwrap(), vec![]).unwrap();
    task.task_id = id;
    let mut state = State::new(8);
    assert!(
        task.update(10, 0, &mut CoreHost::new(&mut state), 100)
            .unwrap()
    );
    task
}

fn complete(mut call: impl FnMut() -> Result<bool>) -> Result<()> {
    for _ in 0..100 {
        if call()? {
            return Ok(());
        }
    }
    Err(Error::new(0, "合成任务未完成本次执行"))
}

#[derive(Default)]
struct World {
    tasks: BTreeMap<u32, OrdinaryTask>,
    bodies: Vec<u32>,
    notifications: Vec<(u32, u32)>,
    released: Vec<u32>,
    inject_parent: Option<(u32, u32)>,
    insert_siblings: Option<(u32, u32)>,
}

impl Lifecycle for World {
    fn state(&mut self, id: u32) -> Result<(&mut Control, &mut TaskClock)> {
        let task = self.tasks.get_mut(&id).unwrap();
        Ok((&mut task.control, &mut task.clock))
    }

    fn update_body(&mut self, tree: &mut TaskTree, id: u32, increment: u32) -> Result<()> {
        if let Some((trigger, parent)) = self.insert_siblings {
            if id == trigger {
                self.insert_siblings = None;
                for id in [0, 3] {
                    tree.insert(parent, id, 12)?;
                    self.tasks.insert(id, task(id));
                }
            }
        }
        if let Some((child, parent)) = self.inject_parent {
            if id == child {
                let task = self.tasks.get_mut(&parent).unwrap();
                assert_eq!(task.clock.time, 100);
                task.clock.paused = -1;
                task.clock.one_tick_pending = true;
                task.enqueue_message([child, 0, 0, 0])?;
                self.inject_parent = None;
            }
        }
        self.bodies.push(id);
        let task = self.tasks.get_mut(&id).unwrap();
        let mut state = State::new(8);
        let mut host = CoreHost::new(&mut state);
        complete(|| task.update_body(increment, &mut host, 1))
    }

    fn request_delete(&mut self, tree: &mut TaskTree, command: Command) -> Result<()> {
        assert!(tree.node(command.sender).is_some());
        assert!(self.tasks.contains_key(&command.sender));
        assert_eq!(
            tree.find_in_subtree(tree.root_id(), command.sender)
                .is_some(),
            command.sender == tree.root_id(),
        );
        assert!(
            tree.find_in_subtree(tree.root_id(), command.target)
                .is_some()
        );
        let task = self.tasks.get_mut(&command.target).unwrap();
        if task.control.deletion_requested == 0 {
            self.notifications.push((command.sender, command.target));
        }
        let kind = tree.node(command.target).unwrap().kind;
        let mut state = State::new(8);
        let mut host = CoreHost::new(&mut state);
        complete(|| {
            task.dispatch_control(kind, &command, &mut 0, &mut host, 1)
                .map(|outcome| outcome.done)
        })
    }

    fn mark_destroying(&mut self, _tree: &mut TaskTree, id: u32) -> Result<()> {
        let task = self.tasks.get_mut(&id).unwrap();
        let mut state = State::new(8);
        let mut host = CoreHost::new(&mut state);
        complete(|| task.request_delete(&mut host, 1))
    }

    fn release(&mut self, id: u32) -> Result<()> {
        let task = self.tasks.remove(&id).unwrap();
        assert!(!task.is_pending());
        self.released.push(id);
        Ok(())
    }
}

#[test]
fn tree_updates_parent_clock_before_children_and_preserves_their_effects_for_its_body() {
    let mut tree = TaskTree::new(1, 12);
    tree.insert(1, 2, 12).unwrap();
    let mut world = World {
        tasks: BTreeMap::from([(1, task(1)), (2, task(2))]),
        inject_parent: Some((2, 1)),
        ..Default::default()
    };
    assert!(tree.update(100, 7, &mut world).unwrap());
    assert_eq!(world.bodies, [2, 1]);
    let parent = &world.tasks[&1];
    assert_eq!(parent.interpreter.read(4, 4).unwrap(), 100u32.to_le_bytes());
    assert!(parent.clock.one_tick_pending);
    assert!(parent.queued_messages().is_empty());
    assert_eq!(parent.accumulated_increment, 7);
    assert!(tree.update(200, 5, &mut world).unwrap());
    let parent = &world.tasks[&1];
    assert_eq!(parent.interpreter.read(4, 4).unwrap(), 0u32.to_le_bytes());
    assert!(!parent.clock.one_tick_pending);
    assert_eq!(parent.accumulated_increment, 12);
}

#[test]
fn deleting_parent_updates_children_then_notifies_siblings_before_releasing_subtrees() {
    let mut tree = TaskTree::new(9, 12);
    for (parent, id) in [(9, 0), (0, 1), (0, 2), (1, 3)] {
        tree.insert(parent, id, 12).unwrap();
    }
    let mut world = World {
        tasks: [9, 0, 1, 2, 3]
            .into_iter()
            .map(|id| (id, task(id)))
            .collect(),
        ..Default::default()
    };
    world.tasks.get_mut(&0).unwrap().control.deletion_requested = -1;
    assert!(tree.update(100, 7, &mut world).unwrap());
    assert_eq!(world.bodies, [3, 1, 2, 9]);
    assert_eq!(world.notifications, [(0, 1), (0, 2), (1, 3)]);
    assert_eq!(world.released, [3, 1, 2, 0]);
    assert_eq!(tree.next_id(), 0);
    assert_eq!(world.tasks.len(), 1);
    tree.insert(9, 4, 12).unwrap();
    world.tasks.insert(4, task(4));
    world.tasks.get_mut(&9).unwrap().control.deletion_requested = 1;
    assert!(!tree.update(200, 5, &mut world).unwrap());
    assert_eq!(world.notifications.last(), Some(&(9, 4)));
    assert_eq!(&world.released[4..], &[4, 9]);
    assert!(world.tasks.is_empty());
    assert!(tree.node(9).is_none());
    assert!(tree.update(300, 5, &mut world).is_err());
}

#[test]
fn inserted_siblings_follow_the_current_update_cursor() {
    let mut tree = TaskTree::new(9, 12);
    tree.insert(9, 1, 12).unwrap();
    let mut world = World {
        tasks: BTreeMap::from([(9, task(9)), (1, task(1))]),
        insert_siblings: Some((1, 9)),
        ..Default::default()
    };
    assert!(tree.update(100, 7, &mut world).unwrap());
    assert_eq!(world.bodies, [1, 3, 9]);
    assert_eq!(world.tasks[&0].clock.time, 10);
    assert_eq!(world.tasks[&3].clock.time, 100);
    world.bodies.clear();
    assert!(tree.update(200, 5, &mut world).unwrap());
    assert_eq!(world.bodies, [0, 1, 3, 9]);
    assert_eq!(world.tasks[&0].accumulated_increment, 5);
    assert_eq!(world.tasks[&3].accumulated_increment, 12);
}
