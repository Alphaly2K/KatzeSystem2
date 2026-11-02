use katze_system2::audio::{LoopPoints, parse_loop_table};

#[test]
fn loop_table_preserves_byte_names_comment_line_boundaries_and_numeric_rules() {
    let points = parse_loop_table(
        b"\xef\xbb\xbfhead\r\n\"a/*x*/b\" \"0x8000000000000000\" 0x10000000000000001\n\
        \"space name\" 1 2\n// omitted\rperiod 1.5 1e3\n\
        split 7 /* block\nend */ 9\nquote\"//hidden 2 3\n\
        a\t18446744073709551615\t-1\n\0ignored 3 4",
    )
    .unwrap();
    let rows: Vec<_> = points
        .iter()
        .map(|p| (p.name.as_slice(), p.start, p.end))
        .collect();
    assert_eq!(
        rows,
        [
            (b"\xef\xbb\xbfhead".as_slice(), 0, -1),
            (b"ab", i64::MIN, 1),
            (b"space", 0, 1),
            (b"period", 0, 1),
            (b"split", 7, -1),
            (b"9", 0, -1),
            (b"quote", 0, -1),
            (b"a", -1, -1),
        ]
    );
    let mut loops = LoopPoints::default();
    for point in points {
        loops.register(&point.name, point.start, point.end).unwrap();
    }
    assert!(loops.find(b"9").is_some());
    assert_eq!(loops.find(b"AB").unwrap().start, i64::MIN);
    assert_eq!(loops.find(b"period").unwrap().end, 1);
    assert!(parse_loop_table(b"/* unfinished").unwrap().is_empty());
}

#[test]
fn duplicate_loop_registration_keeps_the_first_endpoint_name_and_order() {
    let mut loops = LoopPoints::default();
    loops.register(b"music/A", 0, 20).unwrap();
    loops.register(b"music/B", 5, -1).unwrap();
    loops.register(b"MUSIC/a\0ignored", 30, 99).unwrap();
    let point = loops.find(b"music/a").unwrap();
    assert_eq!(point.name, b"music/A");
    assert_eq!((point.start, point.end), (30, 20));
    assert_eq!(loops.entries()[1].name, b"music/B");
    assert_eq!(loops.entries().len(), 2);
    loops.register(b"music/a", 7, 7).unwrap();
    loops.register(b"music/B", 10, 2).unwrap();
    assert_eq!(loops.find(b"music/a").unwrap().start, 30);
    assert_eq!(loops.find(b"music/b\0tail").unwrap().start, 5);
}

#[test]
fn loop_ranges_keep_signed_64_bit_limits_and_validate_only_accepted_names() {
    let mut loops = LoopPoints::default();
    loops.register(b"from-min", i64::MIN, i64::MAX).unwrap();
    loops.register(b"to-end", i64::MAX, -1).unwrap();
    loops.register(b"negative", -2, -1).unwrap();
    loops.register(b"invalid", i64::MAX, i64::MIN).unwrap();
    assert_eq!(loops.find(b"from-min").unwrap().start, i64::MIN);
    assert_eq!(loops.find(b"to-end").unwrap().start, i64::MAX);
    assert_eq!(loops.find(b"negative").unwrap().end, -1);
    assert!(loops.find(b"invalid").is_none());
    let before = loops.entries().to_vec();
    loops.register(&[b'A'; 32], 0, 0).unwrap();
    assert!(loops.register(&[b'A'; 32], 0, 1).is_err());
    assert_eq!(loops.entries(), before);
    loops.register(&[b'A'; 31], 0, 1).unwrap();
    assert_eq!(loops.entries().last().unwrap().name.len(), 31);
}
