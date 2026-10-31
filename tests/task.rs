use katze_system2::{
    expression::State,
    kcs::{CoreHost, Host, Interpreter, NativeReturn, Program},
    task::TaskTree,
};

#[test]
fn tree_orders_signed_ids_and_reuses_ids_after_subtree_destruction() {
    let mut tree = TaskTree::new(4, 1);
    tree.insert(4, u32::MAX, 2).unwrap();
    tree.insert(4, 0x8000_0000, 3).unwrap();
    let a = tree.insert_auto(4, 0).unwrap();
    let b = tree.insert_auto(a, 6).unwrap();
    let c = tree.insert_auto(b, 2).unwrap();
    assert_eq!([a, b, c], [0, 1, 2]);
    assert_eq!(
        tree.children(4).unwrap().collect::<Vec<_>>(),
        [0x8000_0000, u32::MAX, a]
    );
    assert_eq!(
        tree.subtree_ids(4).unwrap(),
        [4, 0x8000_0000, u32::MAX, a, b, c]
    );
    assert_eq!(tree.find_in_subtree(a, c).unwrap().kind, 2);
    assert!(tree.find_in_subtree(b, a).is_none());
    assert!(tree.find_in_subtree(a, u32::MAX).is_none());
    assert!(tree.insert(c, a, 9).is_err());
    assert!(tree.insert(999, 10, 9).is_err());
    assert!(tree.remove(4).is_err());
    assert_eq!(tree.remove(b).unwrap(), [b, c]);
    assert!(tree.find_in_subtree(4, c).is_none());
    assert_eq!(tree.next_id(), 3);
    tree.sweep();
    assert_eq!(tree.insert_auto(u32::MAX, 7).unwrap(), b);
    assert_eq!(tree.insert_auto(a, 0).unwrap(), c);
    assert_eq!(tree.next_id(), 3);
    assert_eq!(tree.node(b).unwrap().parent, Some(u32::MAX));
}

#[test]
fn ordinary_host_enumerates_all_descendant_ids_and_reads_parent_ids() {
    let mut tree = TaskTree::new(u32::MAX, 1);
    tree.insert(u32::MAX, 12, 2).unwrap();
    tree.insert(u32::MAX, 7, 2).unwrap();
    tree.insert(u32::MAX, 0x8000_0000, 2).unwrap();
    tree.insert(12, 0, 6).unwrap();
    let mut state = State::new(8);
    let mut host = CoreHost::new(&mut state);
    let mut vm =
        Interpreter::new(Program::from_code(&[0x3b], vec![], 16).unwrap(), vec![]).unwrap();
    assert!(host.call(75, &0u32.to_le_bytes(), &mut vm).is_err());
    host.task_tree = Some(&tree);
    host.task_id = tree.root_id();
    for (index, expected) in [
        (0, 0x8000_0000),
        (1, 0),
        (2, 7),
        (3, 12),
        (4, u32::MAX),
        (u32::MAX, u32::MAX),
    ] {
        assert_eq!(
            host.call(75, &index.to_le_bytes(), &mut vm).unwrap(),
            NativeReturn::Value(expected)
        );
    }
    assert!(host.call(76, &[], &mut vm).is_err());
    for (id, parent) in [(12, u32::MAX), (0, 12)] {
        host.task_id = id;
        assert_eq!(
            host.call(76, &[], &mut vm).unwrap(),
            NativeReturn::Value(parent)
        );
    }
    host.task_id = 0;
    assert_eq!(
        host.call(75, &0u32.to_le_bytes(), &mut vm).unwrap(),
        NativeReturn::Value(u32::MAX)
    );
    host.task_id = 999;
    assert!(host.call(75, &0u32.to_le_bytes(), &mut vm).is_err());
}

#[test]
fn unregister_keeps_descendant_aliases_and_internal_routes_until_destruction() {
    use katze_system2::task::Command;
    let mut tree = TaskTree::new(9, 1);
    for (parent, id) in [(9, 0), (0, 1), (1, 2), (9, 3)] {
        tree.insert(parent, id, 12).unwrap();
    }
    tree.unregister(0).unwrap();
    assert!(tree.node(0).is_some());
    assert!(tree.find_in_subtree(9, 0).is_none());
    assert!(tree.find_in_subtree(0, 0).is_some());
    assert!(tree.find_in_subtree(9, 2).is_some());
    assert_eq!(tree.children(9).unwrap().collect::<Vec<_>>(), [3]);
    assert_eq!(tree.descendants(9).unwrap().collect::<Vec<_>>(), [1, 2, 3]);
    let mut output = 77;
    let mut calls = Vec::new();
    let mut command = Command {
        target: 0,
        sender: 99,
        code: 0x7000,
        data: [0; 3],
    };
    assert!(
        !tree
            .dispatch(9, command, &mut output, |_, _, _| unreachable!())
            .unwrap()
    );
    assert_eq!(output, 77);
    assert!(
        tree.dispatch(2, command, &mut output, |_, c, _| {
            calls.push(c.target);
            Ok(false)
        })
        .unwrap()
    );
    assert_eq!(calls, [0]);
    calls.clear();
    command.target = 2;
    tree.broadcast(9, None, command, &mut output, |_, c, _| {
        calls.push(c.target);
        Ok(false)
    })
    .unwrap();
    assert_eq!(calls, [0, 1, 2]);
    calls.clear();
    command.target = 999;
    tree.broadcast(9, None, command, &mut output, |_, c, _| {
        calls.push(c.target);
        Ok(false)
    })
    .unwrap();
    assert_eq!(calls, [9, 3]);
    tree.unregister(1).unwrap();
    assert_eq!(tree.subtree_ids(0).unwrap(), [0, 1, 2]);
    assert_eq!(tree.remove(0).unwrap(), [0, 1, 2]);
    assert_eq!(tree.next_id(), 4);
    tree.sweep();
    assert_eq!(tree.next_id(), 0);
}
