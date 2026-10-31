use katze_system2::task::{Command, TaskTree};

fn tree() -> TaskTree {
    let mut tree = TaskTree::new(u32::MAX, 0);
    for (parent, id, kind) in [
        (u32::MAX, 10, 1),
        (u32::MAX, (-2i32) as u32, 2),
        ((-2i32) as u32, 7, 1),
        (10, 3, 2),
        (3, 8, 1),
        (10, 15, 1),
    ] {
        tree.insert(parent, id, kind).unwrap();
    }
    tree
}

fn command(target: u32) -> Command {
    Command {
        target,
        sender: 123,
        code: 173,
        data: [4, 5, u32::MAX],
    }
}

#[test]
fn direct_dispatch_stops_one_branch_and_keeps_other_receivers() {
    let mut tree = tree();
    let mut visited = Vec::new();
    let mut output = 0;
    assert!(
        tree.dispatch(8, command(u32::MAX), &mut output, |_, command, output| {
            visited.push(command.target);
            assert_eq!(command.sender, 123);
            assert_eq!(command.data, [4, 5, u32::MAX]);
            *output = command.target;
            Ok(command.target != 3)
        })
        .unwrap()
    );
    assert_eq!(visited, [u32::MAX, (-2i32) as u32, 7, 10, 3, 15]);
    assert_eq!(output, 15);
    assert!(
        !tree
            .dispatch(8, command(999), &mut output, |_, _, _| unreachable!())
            .unwrap()
    );
    assert_eq!(output, 15);
}

#[test]
fn broadcast_selects_scope_through_ancestor_routing_and_ignores_branch_stops() {
    let mut tree = tree();
    let mut visited = Vec::new();
    let mut output = 0;
    assert!(
        !tree
            .broadcast(u32::MAX, None, command(8), &mut output, |_, command, _| {
                visited.push(command.target);
                assert_eq!(command.data, [4, 5, 0]);
                Ok(false)
            })
            .unwrap()
    );
    assert_eq!(visited, [10, 3, 8, 15]);

    visited.clear();
    assert!(
        tree.broadcast(
            8,
            Some(2),
            command((-2i32) as u32),
            &mut output,
            |_, command, _| {
                visited.push(command.target);
                Ok(false)
            }
        )
        .unwrap()
    );
    assert_eq!(visited, [(-2i32) as u32, 3]);

    visited.clear();
    tree.broadcast(8, None, command(999), &mut output, |_, command, _| {
        visited.push(command.target);
        Ok(true)
    })
    .unwrap();
    assert_eq!(visited, [u32::MAX, (-2i32) as u32, 7, 10, 3, 8, 15]);
}

#[test]
fn live_registration_respects_the_cursor_and_destroyed_siblings_are_skipped() {
    let mut tree = TaskTree::new(u32::MAX, 0);
    for (parent, id) in [(u32::MAX, 10), (u32::MAX, 20), (10, 5), (10, 30)] {
        tree.insert(parent, id, 0).unwrap();
    }
    let mut visited = Vec::new();
    tree.dispatch(5, command(u32::MAX), &mut 0, |tree, command, _| {
        visited.push(command.target);
        if command.target == 5 {
            for (parent, id) in [(u32::MAX, 15), (u32::MAX, 7), (10, 1), (10, 8)] {
                tree.insert(parent, id, 0)?;
            }
            tree.remove(30)?;
        }
        Ok(true)
    })
    .unwrap();
    assert_eq!(visited, [u32::MAX, 10, 5, 8, 15, 20]);
    visited.clear();
    tree.dispatch(5, command(u32::MAX), &mut 0, |_, command, _| {
        visited.push(command.target);
        Ok(true)
    })
    .unwrap();
    assert_eq!(visited, [u32::MAX, 7, 10, 1, 5, 8, 15, 20]);
}
