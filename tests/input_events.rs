use katze_system2::{
    Result,
    input::{Events, HitMask, KeyEvent, KeyEventKind, Regions},
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
