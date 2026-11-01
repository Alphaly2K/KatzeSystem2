use katze_system2::{
    Result,
    graphics::Rect,
    input::Regions,
    task::{Command, CommandBuffers},
};

fn dispatch(
    regions: &mut Regions,
    code: u32,
    a: u32,
    b: u32,
    data: Option<&mut [u8]>,
    output: &mut u32,
) -> Result<bool> {
    regions.dispatch_command(
        &Command {
            target: 8,
            sender: 99,
            code,
            data: [a, b, 0],
        },
        &mut CommandBuffers {
            data0: None,
            data1: data,
        },
        output,
    )
}

fn register(regions: &mut Regions, id: u32, priority: i32, value: u32) {
    let mut data: Vec<_> = [
        id,
        123,
        value,
        (-2i32) as u32,
        3,
        7,
        9,
        0xffffffff,
        8,
        9,
        0x87654321,
        11,
        12,
        13,
        14,
    ]
    .into_iter()
    .flat_map(u32::to_le_bytes)
    .collect();
    let before = data.clone();
    let mut output = 0x13579bdf;
    assert!(
        dispatch(
            regions,
            127,
            priority as u32,
            0,
            Some(&mut data),
            &mut output
        )
        .unwrap()
    );
    assert_eq!(output, 0x13579bdf);
    assert_eq!(data, before);
}

#[test]
fn duplicate_regions_keep_stable_signed_order_and_priority_changes_reappend() {
    let mut regions = Regions::default();
    for (id, priority, value) in [
        (7, -1, 1),
        (3, 5, 2),
        (7, 5, 3),
        (0, i32::MIN, 4),
        (2, 5, 5),
    ] {
        register(&mut regions, id, priority, value);
    }
    let order = |r: &Regions| {
        r.records()
            .iter()
            .map(|r| (r.id(), r.words()[2]))
            .collect::<Vec<_>>()
    };
    assert_eq!(order(&regions), [(3, 2), (7, 3), (2, 5), (7, 1), (0, 4)]);
    assert!(!regions.geometry_dirty());
    let mut output = 0x13579bdf;
    dispatch(&mut regions, 132, 7, 99, None, &mut output).unwrap();
    dispatch(&mut regions, 129, 7, 5, None, &mut output).unwrap();
    assert_eq!(order(&regions), [(3, 2), (2, 5), (7, 99), (7, 1), (0, 4)]);

    dispatch(&mut regions, 133, 7, 0x80000000, None, &mut output).unwrap();
    assert_eq!(
        regions
            .records()
            .iter()
            .map(|r| r.selected())
            .collect::<Vec<_>>(),
        [0, 0, 0x80000000, 0x80000000, 0]
    );
    dispatch(&mut regions, 134, 7, 0, None, &mut output).unwrap();
    assert_eq!(output, 0x80000000);
    dispatch(&mut regions, 133, 3, 9, None, &mut output).unwrap();
    dispatch(&mut regions, 133, 7, 0, None, &mut output).unwrap();
    assert_eq!(regions.get(3).unwrap().selected(), 9);
    dispatch(&mut regions, 128, 999, 7, None, &mut output).unwrap();
    assert_eq!(regions.get(7).unwrap().words()[2], 1);
    assert!(!regions.geometry_dirty());
    dispatch(&mut regions, 133, 999, 1, None, &mut output).unwrap();
    assert!(regions.records().iter().all(|r| r.selected() == 0));
    let before = output;
    dispatch(&mut regions, 134, 999, 0, None, &mut output).unwrap();
    assert_eq!(output, before);
}

#[test]
fn region_geometry_queries_preserve_optional_buffers_and_missing_target_order() {
    let mut regions = Regions::default();
    register(&mut regions, 7, -5, 42);
    assert_eq!(
        regions.get(7).unwrap().words(),
        &[
            7,
            (-5i32) as u32,
            42,
            (-2i32) as u32,
            3,
            7,
            9,
            0,
            1,
            1,
            0x87654321,
            0,
            0,
            0,
            1,
        ]
    );
    assert_eq!(
        regions.get(7).unwrap().bounds(),
        Rect {
            left: -2,
            top: 3,
            right: 7,
            bottom: 9
        }
    );
    let mut output = 0x13579bdf;
    let mut buffer = [0xcc; 20];
    dispatch(&mut regions, 131, 7, 0, Some(&mut buffer), &mut output).unwrap();
    assert_eq!(&buffer[16..], &[0xcc; 4]);
    dispatch(&mut regions, 131, 7, 0, None, &mut output).unwrap();
    assert!(dispatch(&mut regions, 131, 7, 1, None, &mut output).is_err());
    let mut short = [0xcc; 15];
    assert!(dispatch(&mut regions, 131, 7, 0, Some(&mut short), &mut output).is_err());
    assert_eq!(short, [0xcc; 15]);
    dispatch(&mut regions, 131, 999, 1, None, &mut output).unwrap();
    dispatch(&mut regions, 130, 7, 0, None, &mut output).unwrap();
    assert_eq!(regions.get(7).unwrap().bounds(), Rect::default());
    assert!(regions.geometry_dirty());
    regions.clear_geometry_dirty();
    assert!(!regions.geometry_dirty());
    for code in [130, 135, 136] {
        dispatch(&mut regions, code, 999, 1, None, &mut output).unwrap();
        assert!(regions.geometry_dirty());
        regions.clear_geometry_dirty();
    }
    dispatch(&mut regions, 135, 7, 0x80000000, None, &mut output).unwrap();
    assert_eq!(regions.get(7).unwrap().enabled(), 0x80000000);
    let mut short = [0; 59];
    assert!(dispatch(&mut regions, 127, 1, 0, Some(&mut short), &mut output).is_err());
    assert_eq!(regions.records().len(), 1);
    assert!(!dispatch(&mut regions, 137, 7, 1, None, &mut output).unwrap());
    assert_eq!(output, 0x13579bdf);
}
