use katze_system2::kcs::{MotionHost, MotionQueue, MotionState, MotionTask, Program};

fn ins(code: &mut Vec<u8>, opcode: u8, args: &[(u8, u32)]) {
    code.push(opcode);
    for &(flags, value) in args {
        code.push(flags);
        code.extend(value.to_le_bytes());
    }
}

fn assign(code: &mut Vec<u8>, address: u32, flags: u8, value: u32) {
    ins(code, 0x15, &[(3, address), (flags, value)]);
    code.push(0x2d);
}

fn task(factor: f32, offset: f32, duration: u32, delay: u32) -> MotionTask {
    let mut code = vec![];
    ins(&mut code, 0x16, &[(3, 0)]);
    code.push(0x2d);
    assign(&mut code, 4, 0x13, 0x8000_0090);
    ins(&mut code, 3, &[(0x14, 0x8000_0000), (0x14, 0x8000_009c)]);
    ins(&mut code, 1, &[(0x44, 0), (0x14, 0x8000_00a0)]);
    assign(&mut code, 0x8000_0000, 0x44, 0);
    ins(&mut code, 0x25, &[(0x13, 0x8000_0098)]);
    let finishing = code.len();
    ins(&mut code, 0x28, &[(3, 0)]);
    ins(&mut code, 0x11, &[(0x13, 0x8000_0090), (0x13, 0x8000_00a4)]);
    let elapsed = code.len();
    ins(&mut code, 0x28, &[(3, 0)]);
    code.push(0x3b);
    let end = code.len() as u32;
    ins(&mut code, 0x2b, &[(3, 38), (3, 0)]);
    code.push(0x3b);
    for branch in [finishing, elapsed] {
        code[branch + 2..branch + 6].copy_from_slice(&end.to_le_bytes());
    }
    MotionTask::new(
        Program::from_code(&code, vec![0; 32], 256).unwrap(),
        MotionState::with_parameters(&[factor, offset, f32::from_bits(duration)]).unwrap(),
        delay,
    )
    .unwrap()
}

fn counter(queue: &MotionQueue, id: u32) -> u32 {
    u32::from_le_bytes(
        queue
            .task(id)
            .unwrap()
            .interpreter
            .read(0, 4)
            .unwrap()
            .try_into()
            .unwrap(),
    )
}

#[test]
fn creation_order_and_object_cleanup_keep_completed_tasks_until_every_task_stops() {
    let mut queue = MotionQueue::default();
    let mut host = MotionHost::default();
    queue.base_properties[0] = 1.0f32.to_bits();
    let first = queue.insert(task(2.0, 0.5, 3, 0), 0).unwrap();
    let second = queue.insert(task(1.0, -1.25, 7, 0), 1).unwrap();
    for now in 0..=5 {
        assert!(queue.update_object(now, &mut host, 100).unwrap());
        assert_eq!(queue.properties()[0], 1.25f32.to_bits());
    }
    assert_eq!(queue.time_origin(first), Some(1));
    assert_eq!(queue.len(), 2);
    assert!(!queue.any_running(Some(0)).unwrap());
    assert!(queue.any_running(Some(1)).unwrap());
    assert_eq!(counter(&queue, first), 6);
    assert_eq!(counter(&queue, second), 6);
    assert!(queue.update_object(8, &mut host, 100).unwrap());
    assert!(queue.is_empty());
    assert!(queue.was_updated());
    assert_eq!(queue.properties()[0], 1.25f32.to_bits());
    assert_eq!(queue.insert(task(1.0, -1.25, 7, 0), 1).unwrap(), 3);
    queue.insert(task(2.0, 0.5, 3, 0), 0).unwrap();
    assert!(queue.update_object(10, &mut host, 100).unwrap());
    assert_eq!(queue.properties()[0], 0.0f32.to_bits());
}

#[test]
fn delay_skip_and_budget_resume_preserve_the_batch_time_and_composed_input() {
    let mut queue = MotionQueue::default();
    let mut host = MotionHost::default();
    queue.base_properties[0] = 1.0f32.to_bits();
    queue.speed = 0.5;
    let id = queue.insert(task(2.0, 0.5, 3, 3), 0).unwrap();
    for (now, skip) in [(0, false), (10, true), (11, false), (15, false)] {
        assert!(queue.update(now, skip, &mut host, 100).unwrap());
    }
    assert_eq!(queue.time_origin(id), Some(11));
    assert_eq!(counter(&queue, id), 0);
    assert!(!queue.update(17, false, &mut host, 1).unwrap());
    assert!(queue.is_pending());
    assert!(queue.remove(id).is_err());
    assert!(queue.insert(task(1.0, 0.0, 1, 0), 0).is_err());
    queue.speed = 100.0;
    queue.base_properties[0] = 99.0f32.to_bits();
    assert!(queue.update(999, true, &mut host, 100).unwrap());
    assert_eq!(queue.properties()[0], 2.5f32.to_bits());
    assert_eq!(queue.task(id).unwrap().state().unwrap().elapsed, 0);
    assert_eq!(counter(&queue, id), 1);
    queue.set_time_origin(None, 0).unwrap();
    assert!(queue.update(u32::MAX, false, &mut host, 100).unwrap());
    assert_eq!(queue.time_origin(id), Some(0));
}

#[test]
fn grouped_finish_requests_and_budgeted_finish_keep_controls_and_monotonic_ids() {
    let mut queue = MotionQueue::default();
    let mut host = MotionHost::default();
    queue.base_properties[0] = 1.0f32.to_bits();
    let first = queue.insert(task(2.0, 0.5, 3, 100), 0).unwrap();
    let second = queue.insert(task(1.0, -1.25, 7, 100), 1).unwrap();
    assert_eq!(queue.request_finish_group(Some(0)).unwrap(), 1);
    assert!(queue.update(1, false, &mut host, 100).unwrap());
    let before_first = queue.task(first).unwrap().state().unwrap();
    let before_second = queue.task(second).unwrap().state().unwrap();
    assert_eq!(before_first.elapsed, 0u32.wrapping_sub(100));
    assert!(!queue.any_running(Some(0)).unwrap());
    assert!(queue.any_running(Some(1)).unwrap());
    assert!(!queue.finish(&mut host, 1).unwrap());
    assert!(queue.update(200, false, &mut host, 100).is_err());
    assert!(queue.finish(&mut host, 100).unwrap());
    for (id, before) in [(first, before_first), (second, before_second)] {
        let after = queue.task(id).unwrap().state().unwrap();
        assert_eq!(
            (after.elapsed, after.running, after.finishing),
            (before.elapsed, before.running, before.finishing)
        );
    }
    assert_eq!(queue.properties()[0], 1.25f32.to_bits());
    assert_eq!(queue.remove_group(0).unwrap(), 1);
    assert!(queue.request_finish(second).unwrap());
    queue.clear();
    assert!(!queue.is_pending());
    assert_eq!(queue.insert(task(1.0, 0.0, 1, 0), 0).unwrap(), 3);
}
