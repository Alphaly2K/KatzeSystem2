use katze_system2::{
    Error, Result,
    expression::State,
    fes::{ObjectReference, Program},
    runtime::{BaseObject, ObjectFactory, Runtime, TaskObject, fes::Scene, kind},
};

struct Reject;
impl ObjectFactory for Reject {
    fn create(&mut self, _: u32, _: u32, _: &[u8]) -> Result<Box<dyn TaskObject>> {
        Err(Error::new(0, "合成字符串场景不创建下层任务"))
    }
}

fn setup(source: &[u8], budget: usize) -> (Runtime<Reject>, Scene) {
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(16),
        Reject,
    );
    runtime.set_budget(budget, 10000);
    let scene = Scene::new(
        Program::from_text(source).unwrap(),
        runtime.host().handle_allocator(),
    )
    .unwrap();
    (runtime, scene)
}

fn update(runtime: &mut Runtime<Reject>, scene: &mut Scene) -> Result<()> {
    for _ in 0..256 {
        if runtime.update_fes_scene(0, scene, 100)? {
            return Ok(());
        }
    }
    panic!("合成字符串场景没有完成");
}

#[test]
fn string_comparison_and_range_clearing_share_main_system_and_event_state() {
    let source = b"#MACRO\nCMP strcmp\n#DEFINE\nBASE obj[2]\n#START\nstrcmp\nCMP ($0+=1) (:raw\xff) (:raw\x80) ($2+=100)\nstrcmp 2 A a\nstrcmp 3 aa a\nstrcmp 4 (:source) $str0;\nstrcmp 5\nstrcmp 6 x\nstrcmp 9 ($7+=100) x\nstrcmp -1 z a\nstrcmp 16 z a\nclearstrvar\nclearstrvar ($7+=10) ($8+=11) ($9+=100)\nclearstrvar 8 6\nclearstrvar -2 -1\nwait 99\n#_FES_SLEEP_\nstrcmp 12 $str4; before\nclearstrvar 4\n#obj.PUSH_L\nstrcmp (13+@) (:raw\x82\xa0) (:raw\x82\xa0)\nclearstrvar 0\n";
    for budget in [1, 4096] {
        let (mut runtime, mut scene) = setup(source, budget);
        for (index, value) in [
            (0, b"source".as_slice()),
            (4, b"before"),
            (8, b"keep"),
            (10, b"remove"),
            (11, b"remove"),
            (u32::MAX - 1, b"remove"),
            (u32::MAX, b"remove"),
        ] {
            runtime.numbers_mut().strings.insert(index, value.to_vec());
        }
        update(&mut runtime, &mut scene).unwrap();
        assert_eq!(
            &runtime.numbers().shared[..10],
            &[1, 1, -1, 1, 0, 0, 1, 10, 11, -1]
        );
        assert_eq!(
            runtime
                .numbers()
                .strings
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            [0, 4, 8]
        );
        let deadline = scene.deadline();
        let mut complete = false;
        for _ in 0..256 {
            if runtime
                .dispatch_fes_system_block(0, &mut scene, "_FES_SLEEP_")
                .unwrap()
            {
                complete = true;
                break;
            }
        }
        assert!(complete);
        assert_eq!(runtime.numbers().shared[12], 0);
        assert!(!runtime.numbers().strings.contains_key(&4));
        runtime.numbers_mut().shared[14] = 99;
        let target = ObjectReference {
            name: "obj".into(),
            bank: 1,
        };
        complete = false;
        for _ in 0..256 {
            if let Some(handled) = runtime
                .dispatch_fes_event(0, &mut scene, Some(&target), 2)
                .unwrap()
            {
                assert!(handled);
                complete = true;
                break;
            }
        }
        assert!(complete);
        assert_eq!(runtime.numbers().shared[14], 0);
        assert_eq!(
            runtime
                .numbers()
                .strings
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            [8]
        );
        assert_eq!(scene.deadline(), deadline);
        assert!(runtime.host().debug_messages.is_empty());

        let (mut runtime, mut scene) = setup(b"#START\nclearstrvar 0 2147483647\n", budget);
        runtime.numbers_mut().strings.insert(4, b"keep".to_vec());
        assert!(update(&mut runtime, &mut scene).is_err());
        assert_eq!(runtime.numbers().strings[&4], b"keep");
    }
}

#[test]
fn strvar_shares_strings_across_main_system_and_object_handlers_without_repeating_expressions() {
    let source = b"#MACRO\nSTR strvar\n#DEFINE\nBASE obj[2]\n#START\nstrvar\nstrvar 2 initial\nstrvar 2\nSTR ($0+=4) (:v=%02d/%d) ($1+=3) ($2-=9) ($3+=4) ($4+=5) ($5+=100)\nstrvar 8 (:%% %d)\nstrvar 10 \"$str4;\" ($6+=100)\nstrvar get 11 4\nstrvar get 12 999\nstrvar -1 literal\nwait 99\n#_FES_SLEEP_\nstrvar GET 1 4\nstrvar 4 (:after=%u) -1\n#obj.PUSH_L\nstrvar get 7 1\nif (1) strvar 6 (:bank=%d) @\n";
    for budget in [1, 4096] {
        let (mut runtime, mut scene) = setup(source, budget);
        update(&mut runtime, &mut scene).unwrap();
        assert_eq!(runtime.numbers().shared[..7], [4, 3, -9, 4, 5, 0, 0]);
        let strings = &runtime.numbers().strings;
        assert!(strings[&2].is_empty());
        assert_eq!(strings[&4], b"v=03/-9");
        assert_eq!(strings[&8], b"%% %d");
        assert_eq!(strings[&10], b"v=03/-9");
        assert_eq!(strings[&11], strings[&4]);
        assert!(strings[&12].is_empty());
        assert_eq!(strings[&u32::MAX], b"literal");
        let deadline = scene.deadline();
        let mut done = false;
        for _ in 0..256 {
            if runtime
                .dispatch_fes_system_block(0, &mut scene, "_FES_SLEEP_")
                .unwrap()
            {
                done = true;
                break;
            }
        }
        assert!(done);
        done = false;
        let target = ObjectReference {
            name: "obj".into(),
            bank: 1,
        };
        for _ in 0..256 {
            if let Some(handled) = runtime
                .dispatch_fes_event(0, &mut scene, Some(&target), 2)
                .unwrap()
            {
                assert!(handled);
                done = true;
                break;
            }
        }
        assert!(done);
        assert_eq!(runtime.numbers().strings[&1], b"v=03/-9");
        assert_eq!(runtime.numbers().strings[&4], b"after=4294967295");
        assert_eq!(runtime.numbers().strings[&7], b"v=03/-9");
        assert_eq!(runtime.numbers().strings[&6], b"bank=1");
        assert_eq!(scene.deadline(), deadline);
    }
}

#[test]
fn strvar_integer_formats_preserve_crt_widths_limits_and_zero_termination() {
    for (format, arguments, expected) in [
        ("%+06d", "-7", b"-00007".as_slice()),
        ("%#08x", "42", b"0x00002a"),
        ("%#.0o", "0", b""),
        ("%*.*d", "-8 3 -7", b"-007    "),
        ("%I64d", "0 -2147483648", b"-9223372036854775808"),
        ("%hd", "65535", b"-1"),
        ("%hhd", "255", b"255"),
        ("%hu", "-1", b"65535"),
        ("%cTAIL", "0", b""),
        ("%05c", "65", b"0000A"),
        ("%d%%", "17", b"17%"),
    ] {
        let source = format!("#START\nstrvar 4 (:{format}) {arguments}\nwait 99\n");
        let (mut runtime, mut scene) = setup(source.as_bytes(), 4096);
        update(&mut runtime, &mut scene).unwrap();
        assert_eq!(runtime.numbers().strings[&4], expected);
    }
    for command in [
        "strvar 4 (:%s) 0".to_owned(),
        "strvar 4 (:%d%d) 1".to_owned(),
        "strvar 4 (:%2560d) 1".to_owned(),
        format!("strvar 4 {}", "x".repeat(2560)),
    ] {
        let source = format!("#START\n{command}\nwait 99\n");
        let (mut runtime, mut scene) = setup(source.as_bytes(), 4096);
        runtime.numbers_mut().strings.insert(4, b"before".to_vec());
        assert!(update(&mut runtime, &mut scene).is_err());
        assert_eq!(runtime.numbers().strings[&4], b"before");
    }
}
