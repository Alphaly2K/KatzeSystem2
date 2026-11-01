use katze_system2::{
    Result,
    input::{Events, HitMask, KeyEvent, KeyEventKind, PointerMotion, Regions},
    task::{Command, CommandBuffers},
};

fn command(regions: &mut Regions, code: u32, a: u32, b: u32) {
    assert!(
        regions
            .dispatch_command(
                &Command {
                    sender: 8,
                    target: 10,
                    code,
                    data: [a, b, 0],
                },
                &mut CommandBuffers::default(),
                &mut 0,
            )
            .unwrap()
    );
}

fn register(regions: &mut Regions, id: u32, priority: i32, mask: u32, rect: [i32; 4]) {
    let mut bytes: Vec<_> = [
        id,
        0,
        mask,
        rect[0] as u32,
        rect[1] as u32,
        rect[2] as u32,
        rect[3] as u32,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
    ]
    .into_iter()
    .flat_map(u32::to_le_bytes)
    .collect();
    regions
        .dispatch_command(
            &Command {
                target: 10,
                sender: 8,
                code: 127,
                data: [priority as u32, 0, 0],
            },
            &mut CommandBuffers {
                data0: None,
                data1: Some(&mut bytes),
            },
            &mut 0,
        )
        .unwrap();
}

fn event(kind: KeyEventKind, pressed: u32, mask: u32, point: [i32; 2]) -> KeyEvent {
    KeyEvent {
        kind,
        code: 80,
        key: 79,
        pressed,
        mask,
        point,
    }
}

#[test]
fn changes_capture_releases_clear_and_repeats_use_only_one_candidate() {
    let mut regions = Regions::default();
    register(&mut regions, 7, 10, 2, [0, 0, 8, 8]);
    register(&mut regions, 9, 5, 2, [0, 0, 8, 8]);
    register(&mut regions, 11, 0, 2, [0, 0, 8, 8]);
    let mut state = Events::default();
    let mut log = Vec::new();
    assert!(
        state
            .dispatch_key(
                10,
                &mut regions,
                event(KeyEventKind::Change, 5, 2, [1, 1]),
                |_, packet| {
                    log.push(packet);
                    Ok(match packet.target {
                        7 => None,
                        9 => Some(99),
                        _ => Some(0),
                    })
                },
            )
            .unwrap()
    );
    assert_eq!(log.iter().map(|p| p.target).collect::<Vec<_>>(), [7, 9, 11]);
    assert!(
        log.iter()
            .all(|p| p.sender == 10 && p.code == 80 && p.data == [79, 5, 0])
    );
    assert_eq!(state.captures()[79], 11);
    assert!(state.captures()[..79].iter().all(|&id| id == 0));
    log.clear();
    assert!(
        !state
            .dispatch_key(
                10,
                &mut regions,
                event(KeyEventKind::Repeat, 1, 2, [8, 1]),
                |_, packet| {
                    log.push(packet);
                    Ok(Some(0))
                },
            )
            .unwrap()
    );
    assert!(log.is_empty());
    assert_eq!(state.captures()[79], 11);
    register(&mut regions, 11, -1, 0, [100, 100, 101, 101]);
    assert!(
        state
            .dispatch_key(
                10,
                &mut regions,
                event(KeyEventKind::Change, 0, 0, [-99, -99]),
                |_, packet| {
                    log.push(packet);
                    Ok(if log.len() == 1 { None } else { Some(0) })
                },
            )
            .unwrap()
    );
    assert_eq!(log.iter().map(|p| p.target).collect::<Vec<_>>(), [11, 11]);
    assert!(log.iter().all(|p| p.data == [79, 0, 0]));
    assert_eq!(state.captures()[79], 0);
    command(&mut regions, 135, 7, 0);
    assert!(
        !state
            .dispatch_key(
                10,
                &mut regions,
                event(KeyEventKind::Repeat, 1, 2, [1, 1]),
                |_, _| panic!("无捕获时重复事件只检查首项"),
            )
            .unwrap()
    );
    log.clear();
    assert!(
        !state
            .dispatch_key(
                10,
                &mut regions,
                event(KeyEventKind::Change, 0, 0, [-99, -99]),
                |_, packet| {
                    log.push(packet);
                    Ok(Some(1))
                },
            )
            .unwrap()
    );
    assert_eq!(
        log.iter().map(|p| p.target).collect::<Vec<_>>(),
        [9, 11, 11]
    );
}

#[test]
fn masks_keep_row_padding_wrapping_origins_and_pointer_filtering() {
    let mask = HitMask::new([9, 2], vec![0x81, 0xff, 0x02, 0x01]).unwrap();
    for (point, expected) in [
        ([0, 0], true),
        ([1, 0], false),
        ([7, 0], true),
        ([8, 0], true),
        ([9, 0], false),
        ([0, 1], false),
        ([1, 1], true),
        ([8, 1], true),
        ([8, 2], false),
        ([-1, 0], false),
    ] {
        assert_eq!(mask.contains([0, 0], point), expected);
    }
    assert!(mask.contains([i32::MAX, i32::MIN], [i32::MIN + 6, i32::MIN]));
    assert!(!mask.contains([i32::MIN, 0], [i32::MAX, 0]));
    assert!(HitMask::new([9, 2], vec![0; 3]).is_err());
    assert!(HitMask::new([u32::MAX, 0], vec![]).is_err());
    assert!(
        !HitMask::new([0, 9], vec![])
            .unwrap()
            .contains([0; 2], [0; 2])
    );

    let mut regions = Regions::default();
    register(&mut regions, 7, 0, 3, [5, 8, 7, 10]);
    assert!(regions.set_hit_mask(7, Some(mask.clone())));
    assert_eq!(&regions.get(7).unwrap().words()[11..14], &[1, 9, 2]);
    let mut state = Events::default();
    let mut dispatch = |regions: &mut Regions, mask, point| {
        state.dispatch_key(
            10,
            regions,
            event(KeyEventKind::Change, 1, mask, point),
            |_, _| Ok(Some(0)),
        )
    };
    assert!(dispatch(&mut regions, 2, [5, 8]).unwrap());
    assert!(!dispatch(&mut regions, 2, [6, 8]).unwrap());
    assert!(!dispatch(&mut regions, 2, [12, 8]).unwrap());
    command(&mut regions, 130, 7, 0);
    assert!(dispatch(&mut regions, 2, [7, 0]).unwrap());
    assert!(dispatch(&mut regions, 1, [-999, -999]).unwrap());
    assert!(regions.set_hit_mask(7, None));
    assert_eq!(&regions.get(7).unwrap().words()[11..14], &[0; 3]);
    assert!(dispatch(&mut regions, 2, [-999, -999]).unwrap());
    assert!(!regions.set_hit_mask(999, Some(mask)));
}

#[test]
fn callbacks_follow_live_order_and_diagnose_removed_current_nodes() {
    let mut regions = Regions::default();
    register(&mut regions, 3, 10, 3, [0, 0, 8, 8]);
    register(&mut regions, 7, 5, 3, [0, 0, 8, 8]);
    register(&mut regions, 9, 0, 3, [0, 0, 8, 8]);
    let mut state = Events::default();
    let mut log = Vec::new();
    assert!(
        state
            .dispatch_key(
                10,
                &mut regions,
                event(KeyEventKind::Change, 1, 3, [1, 1]),
                |regions, packet| {
                    log.push(packet.target);
                    if packet.target == 3 {
                        register(regions, 13, 12, 3, [0; 4]);
                        register(regions, 15, 1, 3, [0; 4]);
                        command(regions, 128, 0, 7);
                        command(regions, 129, 9, 11);
                        Ok(None)
                    } else {
                        Ok(Some(0))
                    }
                },
            )
            .unwrap()
    );
    assert_eq!(log, [3, 15]);
    assert_eq!(state.captures()[79], 15);
    for code in [128, 129] {
        let result = state.dispatch_key(
            10,
            &mut regions,
            event(KeyEventKind::Change, 1, 3, [1, 1]),
            |regions, packet| {
                command(
                    regions,
                    code,
                    if code == 128 { 0 } else { packet.target },
                    if code == 128 { packet.target } else { 0 },
                );
                Ok(Some(1))
            },
        );
        assert!(result.unwrap_err().message.contains("当前区域"));
    }
    assert_eq!(state.captures()[79], 0);
}

#[test]
fn suppression_preserves_state_and_zero_id_keeps_the_uncaptured_sentinel() -> Result<()> {
    let mut regions = Regions::default();
    register(&mut regions, 0, 0, 16, [0; 4]);
    let mut state = Events::default();
    assert!(state.dispatch_key(
        10,
        &mut regions,
        event(KeyEventKind::Change, 1, 16, [0; 2]),
        |_, _| Ok(Some(0)),
    )?);
    assert_eq!(state.captures()[79], 0);
    assert_eq!(state.raw_state(), 16);
    assert_eq!(state.state(), 2);
    state.set_suppressed(5);
    assert!(!state.dispatch_key(
        10,
        &mut regions,
        event(KeyEventKind::Change, 1, 3, [0; 2]),
        |_, _| panic!("被抑制事件不投递"),
    )?);
    assert_eq!(state.raw_state(), 16);
    assert_eq!(state.suppressed(), 5);
    state.set_suppressed(0);
    assert!(!state.dispatch_key(
        10,
        &mut regions,
        event(KeyEventKind::Repeat, 0, 3, [0; 2]),
        |_, _| panic!("重复入口的释放不投递"),
    )?);
    assert_eq!(state.state(), 3);
    let mut invalid = event(KeyEventKind::Change, 1, 99, [0; 2]);
    invalid.key = 80;
    assert!(
        state
            .dispatch_key(10, &mut regions, invalid, |_, _| Ok(Some(0)))
            .is_err()
    );
    assert_eq!(state.state(), 3);
    Ok(())
}

#[test]
fn pointer_selection_uses_independent_geometry_and_orders_flag_notifications() -> Result<()> {
    let mut regions = Regions::default();
    register(&mut regions, 7, 10, 0, [0; 4]);
    register(&mut regions, 9, 0, 0, [0, 0, 2, 2]);
    command(&mut regions, 133, 9, 5);
    regions.update_pointer_selection(10, Some([1, 1]), |_, _| {
        panic!("保持已有选择值时不重复通知")
    })?;
    assert_eq!(regions.get(9).unwrap().selected(), 5);
    regions.set_hit_mask(7, Some(HitMask::new([3, 1], vec![4])?));
    let mut log = Vec::new();
    regions.update_pointer_selection(10, Some([2, 0]), |regions, packet| {
        log.push(packet);
        if packet.code == 8 {
            assert_eq!(regions.get(9).unwrap().selected(), 5);
            assert_eq!(regions.get(7).unwrap().selected(), 0);
            command(regions, 133, 9, 99);
        } else {
            assert_eq!(regions.get(9).unwrap().selected(), 0);
            assert_eq!(regions.get(7).unwrap().selected(), 1);
        }
        Ok(())
    })?;
    assert_eq!(
        log.iter().map(|p| (p.target, p.code)).collect::<Vec<_>>(),
        [(9, 8), (7, 7)]
    );
    assert!(log.iter().all(|p| p.sender == 10 && p.data == [2, 0, 0]));
    command(&mut regions, 136, 7, 0);
    assert_eq!(regions.get(7).unwrap().selection_enabled(), 0);
    regions.clear_geometry_dirty();
    log.clear();
    regions.update_pointer_selection(10, Some([1, 1]), |_, packet| {
        log.push(packet);
        Ok(())
    })?;
    assert_eq!(log.len(), 1);
    assert_eq!((log[0].target, log[0].code), (9, 7));
    assert_eq!(regions.get(7).unwrap().selected(), 1);
    assert_eq!(regions.get(9).unwrap().selected(), 1);
    assert!(!regions.geometry_dirty());
    command(&mut regions, 136, 7, 1);
    log.clear();
    regions.update_pointer_selection(10, None, |_, packet| {
        log.push(packet);
        Ok(())
    })?;
    assert_eq!(
        log.iter().map(|p| (p.target, p.code)).collect::<Vec<_>>(),
        [(7, 8), (9, 8)]
    );
    assert!(
        log.iter()
            .all(|p| p.data == [i32::MIN as u32, i32::MIN as u32, 0])
    );
    assert!(regions.records().iter().all(|r| r.selected() == 0));
    Ok(())
}

#[test]
fn pointer_selection_keeps_duplicate_and_sentinel_rules_and_live_callback_changes() -> Result<()> {
    let mut regions = Regions::default();
    register(&mut regions, 3, 10, 0, [0; 4]);
    command(&mut regions, 135, 3, 0);
    register(&mut regions, 3, 0, 0, [0, 0, 8, 8]);
    regions.update_pointer_selection(10, Some([1, 1]), |regions, packet| {
        assert_eq!(packet.code, 7);
        assert_eq!(packet.target, 3);
        assert_eq!(regions.records()[0].enabled(), 0);
        assert_eq!(regions.records()[0].selected(), 1);
        assert_eq!(regions.records()[1].selected(), 0);
        Ok(())
    })?;

    let mut regions = Regions::default();
    register(&mut regions, u32::MAX, 0, 0, [0, 0, 8, 8]);
    regions
        .update_pointer_selection(10, Some([1, 1]), |_, _| panic!("编号全一与无候选哨兵相同"))?;
    assert_eq!(regions.get(u32::MAX).unwrap().selected(), 0);
    command(&mut regions, 133, u32::MAX, 7);
    regions
        .update_pointer_selection(10, None, |_, _| panic!("无候选时保留同哨兵编号的已有选择"))?;
    assert_eq!(regions.get(u32::MAX).unwrap().selected(), 7);

    let mut regions = Regions::default();
    register(&mut regions, 7, 10, 0, [0; 4]);
    register(&mut regions, 9, 0, 0, [0, 0, 8, 8]);
    command(&mut regions, 133, 7, 1);
    let mut log = Vec::new();
    regions.update_pointer_selection(10, Some([1, 1]), |regions, packet| {
        log.push((packet.target, packet.code));
        command(regions, 133, 9, 7);
        Ok(())
    })?;
    assert_eq!(log, [(7, 8)]);
    assert_eq!(regions.get(7).unwrap().selected(), 0);
    assert_eq!(regions.get(9).unwrap().selected(), 7);
    Ok(())
}

#[test]
fn pointer_motion_shares_mutable_points_and_filters_without_hit_masks() -> Result<()> {
    let mut regions = Regions::default();
    register(&mut regions, 7, 10, 2, [0; 4]);
    register(&mut regions, 9, 9, 2, [5, 5, 8, 8]);
    register(&mut regions, 11, 8, 2, [0; 4]);
    register(&mut regions, 13, 7, 1, [0; 4]);
    register(&mut regions, 15, 6, 2, [1, 1, 2, 2]);
    register(&mut regions, 17, 5, 2, [8, 9, -1, -2]);
    regions.set_hit_mask(7, Some(HitMask::new([1, 1], vec![0])?));
    command(&mut regions, 136, 7, 0);
    command(&mut regions, 154, 11, 0);
    regions.clear_geometry_dirty();
    let motion = PointerMotion {
        code: 6,
        previous: [1, 1],
        current: [99, 99],
    };
    let mut log = Vec::new();
    regions.dispatch_pointer_motion(10, motion, |_, packet, buffers| {
        assert_eq!(packet.sender, 10);
        assert_eq!(packet.code, 6);
        assert_eq!(packet.data, [0; 3]);
        log.push(packet.target);
        let previous = buffers.data0.as_deref_mut().unwrap();
        let current = buffers.data1.as_deref_mut().unwrap();
        if packet.target == 7 {
            assert_eq!(&previous[..4], &1i32.to_le_bytes());
            assert_eq!(&current[..4], &99i32.to_le_bytes());
            previous[..4].copy_from_slice(&5i32.to_le_bytes());
            previous[4..].copy_from_slice(&5i32.to_le_bytes());
            current[..4].copy_from_slice(&(-7i32).to_le_bytes());
            current[4..].copy_from_slice(&17i32.to_le_bytes());
        } else {
            assert_eq!(&previous[..4], &5i32.to_le_bytes());
            assert_eq!(&previous[4..], &5i32.to_le_bytes());
            assert_eq!(&current[..4], &(-7i32).to_le_bytes());
            assert_eq!(&current[4..], &17i32.to_le_bytes());
        }
        Ok(())
    })?;
    assert_eq!(log, [7, 9, 17]);
    assert!(!regions.geometry_dirty());
    register(&mut regions, 7, -1, 2, [0; 4]);
    command(&mut regions, 154, 7, 0);
    command(&mut regions, 154, 999, 1);
    assert_eq!(regions.get(7).unwrap().motion_enabled(), 0);
    assert_eq!(regions.records().last().unwrap().motion_enabled(), 1);
    assert!(!regions.geometry_dirty());
    log.clear();
    regions.dispatch_pointer_motion(
        10,
        PointerMotion {
            previous: [100; 2],
            ..motion
        },
        |_, packet, _| {
            log.push(packet.target);
            Ok(())
        },
    )?;
    assert_eq!(log, [17, 7]);
    assert_eq!(motion.previous, [1, 1]);
    assert_eq!(motion.current, [99, 99]);
    Ok(())
}
