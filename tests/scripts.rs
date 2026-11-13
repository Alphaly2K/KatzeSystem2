use katze_system2::{
    Error, Result, Step, anm, cst, cstl,
    expression::{self, State},
    fes, kcs,
};
use std::{collections::BTreeMap, io::Write, sync::Arc};

fn compressed(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}
fn word(out: &mut Vec<u8>, n: usize) {
    out.extend((n as u32).to_le_bytes());
}

fn scene(records: &[(u8, &[u8])]) -> cst::Scene {
    let mut groups = Vec::new();
    let mut start = 0;
    for (i, (kind, _)) in records.iter().enumerate() {
        if matches!(kind, 2 | 3) {
            groups.push(start..i + 1);
            start = i + 1;
        }
    }
    groups.push(start..records.len());
    let mut strings = Vec::new();
    let mut offsets = Vec::new();
    for &(kind, bytes) in records {
        word(&mut offsets, strings.len());
        strings.extend([1, kind]);
        strings.extend(bytes);
        strings.push(0);
    }
    let mut body = Vec::new();
    word(&mut body, groups.len() * 8 + offsets.len() + strings.len());
    word(&mut body, groups.len());
    word(&mut body, groups.len() * 8);
    word(&mut body, groups.len() * 8 + offsets.len());
    for g in groups {
        word(&mut body, g.len());
        word(&mut body, g.start);
    }
    body.extend(offsets);
    body.extend(strings);
    let mut bytes = b"CatScene".to_vec();
    word(&mut bytes, 0);
    word(&mut bytes, body.len());
    bytes.extend(body);
    cst::Scene {
        script: Arc::new(cst::Script::parse(&bytes).unwrap()),
        localization: None,
    }
}

struct Scenes(BTreeMap<String, cst::Scene>);
impl cst::SceneSource for Scenes {
    fn load(&mut self, name: &str) -> Result<cst::Scene> {
        self.0
            .get(name)
            .cloned()
            .ok_or_else(|| Error::new(0, "缺少合成场景"))
    }
}

#[test]
fn numeric_oracle_precedence_and_integer_edges() {
    let cases = [
        ("1+2*3", 7),
        ("3<2==0", 1),
        ("2==3<1", 0),
        ("1&2==0", 0),
        ("1|2^3", 1),
        ("1|0&&0", 0),
        ("1||0&&0", 1),
        ("2147483647+1", i32::MIN),
        ("0x100000000", 0),
        ("-7/3", -2),
        ("7%-3", 1),
        ("-7%3", -1),
        ("1<<-1", i32::MIN),
        ("1<<33", 2),
        ("-8>>1", -4),
        ("5/0", 0),
        ("5%0", 0),
    ];
    let mut state = State::new(32);
    for (source, expected) in cases {
        assert_eq!(
            expression::eval(source, &mut state).unwrap(),
            expected,
            "{source}"
        );
    }
    assert!(expression::eval("-2147483648/-1", &mut state).is_err());
    assert!(expression::eval("#4/=0", &mut state).is_err());
}

#[test]
fn named_operands_are_read_only_banked_values_and_capture_before_later_effects() {
    let mut state = State::new(8);
    state.named.insert(
        "value".into(),
        BTreeMap::from([(-1, 17), (0, 41), (1, 42), (2, 43)]),
    );
    state
        .named
        .insert("_FRAME_".into(), BTreeMap::from([(0, 123)]));
    for (source, result, slot) in [
        ("value", 41, 0),
        ("VALUE", 0, 0),
        ("missing", 0, 0),
        ("value[-1]", 17, 0),
        ("value[1+1]", 43, 0),
        ("value[99]", 0, 0),
        ("value[$4++]", 42, 1),
        ("missing[$4++]", 0, 1),
        ("value[$4]+($4=1)", 42, 1),
        ("_FRAME_+value", 164, 0),
    ] {
        state.shared[4] = 0;
        assert_eq!(
            expression::eval(source, &mut state).unwrap(),
            result,
            "{source}"
        );
        assert_eq!(state.shared[4], slot, "{source}");
    }
    for source in [
        "value=99", "value+=1", "value++", "++value", "value[]", "value[1", "value.X",
    ] {
        assert!(expression::eval(source, &mut state).is_err(), "{source}");
    }
    assert_eq!(state.named["value"][&0], 41);
}

#[test]
fn numeric_side_effects_keep_locations_and_do_not_short_circuit() {
    let mut state = State::new(32);
    state.shared[4] = 1;
    state.shared[9] = 7;
    assert_eq!(expression::eval("#4+(#4=5)", &mut state).unwrap(), 10);
    assert_eq!(expression::eval("(#4=2)+(#4=3)", &mut state).unwrap(), 5);
    assert_eq!(expression::eval("#4=#9", &mut state).unwrap(), 7);
    assert_eq!(state.shared[4], 7);
    assert_eq!(expression::eval("#4+=#9+=1", &mut state).unwrap(), 15);
    assert_eq!(state.shared[9], 8);
    assert_eq!(expression::eval("#4++", &mut state).unwrap(), 16);
    assert_eq!(expression::eval("0&&++#4", &mut state).unwrap(), 0);
    assert_eq!(state.shared[4], 17);
    expression::eval("1||(#4=9)", &mut state).unwrap();
    assert_eq!(state.shared[4], 9);
    assert_eq!(expression::eval("#(#4)=(#4=10)", &mut state).unwrap(), 10);
    assert_eq!(state.shared[9], 10);
    expression::eval("\\3=8", &mut state).unwrap();
    assert_eq!(state.local[3], 8);
    assert_eq!(state.shared[3], 0);
    for (source, initial, result, slot) in [
        ("+30", 9, 0, 9),
        ("#3=+30", 9, 30, 30),
        ("+(#3+=1)", 30, 0, 31),
        ("+#3*3+4", 31, 97, 31),
        ("+#3++", 31, 0, 31),
    ] {
        state.shared[3] = initial;
        assert_eq!(
            expression::eval(source, &mut state).unwrap(),
            result,
            "{source}"
        );
        assert_eq!(state.shared[3], slot, "{source}");
    }
}

#[test]
fn numeric_invalid_input_is_bounded_and_does_not_run_partial_assignment() {
    let mut state = State::new(8);
    for source in ["#4=1 garbage", "#8", "#(-1)", "(1+2", "0x", "1+", "++1"] {
        assert!(expression::eval(source, &mut state).is_err(), "{source}");
    }
    assert_eq!(state.shared[4], 0);
    assert!(expression::Expression::parse(&"1+".repeat(10000)).is_err());
    assert!(
        expression::Expression::parse(&format!("{}1{}", "(".repeat(1000), ")".repeat(1000)))
            .is_err()
    );
}

#[test]
fn cst_calls_strings_and_tail_commands_resume_once() {
    let main = scene(&[
        (0x30, b"#4=1"),
        (0x30, b"call child"),
        (0x30, b"str 7 (:hello world)"),
        (0x20, b"$str7"),
        (2, b""),
        (0x30, b"#9=$100"),
        (0x30, b"draw (1 + 2) 7"),
    ]);
    for (input, record, expected) in [
        (-1, i32::MAX, 2),
        (i32::MIN, i32::MIN, 2),
        (0, -1, 5),
        (1, i32::MIN, 2),
        (0, 3, 0x20),
        (0, 4, 2),
        (1, 0, 0x30),
        (0, 5, u32::MAX),
        (2, -1, u32::MAX),
        (i32::MAX, 0, u32::MAX),
    ] {
        assert_eq!(main.script.record_kind(input, record), expected);
    }
    let empty = scene(&[]);
    assert_eq!(empty.script.record_kind(-1, 0), 1);
    assert_eq!(empty.script.record_kind(0, -1), 0);
    assert_eq!(empty.script.record_kind(0, 0), u32::MAX);
    let mut source = Scenes(BTreeMap::from([(
        "child".into(),
        scene(&[(0x30, b"#4+=2"), (0x30, b"return")]),
    )]));
    let mut vm = cst::Interpreter::new(main, "en");
    let mut state = State::new(2048);
    let event = vm.run(&mut state, &mut source, 100).unwrap();
    assert_eq!(
        event,
        Step::Event(cst::Event::Text {
            base: "hello world".into(),
            localized: None
        })
    );
    assert_eq!(state.shared[4], 3);
    assert_eq!(vm.run(&mut state, &mut source, 100).unwrap(), event);
    vm.resume().unwrap();
    assert_eq!(
        vm.run(&mut state, &mut source, 100).unwrap(),
        Step::Event(cst::Event::Input { page: false })
    );
    vm.resume().unwrap();
    assert_eq!(
        vm.run(&mut state, &mut source, 100).unwrap(),
        Step::Event(cst::Event::Command(vec![
            b"draw".to_vec(),
            b"(1 + 2)".to_vec(),
            b"7".to_vec()
        ]))
    );
    assert_eq!(state.shared[9], 256);
    vm.resume().unwrap();
    assert_eq!(vm.run(&mut state, &mut source, 100).unwrap(), Step::Halted);
}

#[test]
fn cst_names_do_not_consume_localized_body_fragments() {
    let mut input = scene(&[(0x21, b"N"), (0x20, b"one"), (0x21, b"N"), (0x20, b"two")]);
    input.localization = Some(Arc::new(cstl::Table {
        languages: vec!["en".into()],
        messages: vec![vec![cstl::Message {
            name: "Name".into(),
            content: "first\x1fsecond".into(),
        }]],
    }));
    let mut vm = cst::Interpreter::new(input, "en");
    let mut state = State::new(8);
    let mut source = Scenes(BTreeMap::new());
    for expected in ["Name", "first", "Name", "second"] {
        let Step::Event(event) = vm.run(&mut state, &mut source, 50).unwrap() else {
            panic!()
        };
        let translated = match event {
            cst::Event::Name { localized, .. } | cst::Event::Text { localized, .. } => localized,
            _ => panic!(),
        };
        assert_eq!(translated.as_deref(), Some(expected));
        vm.resume().unwrap();
    }
}

#[test]
fn conditional_choices_use_dense_indices_and_preserve_duplicate_ids() {
    let mut input = scene(&[
        (0x30, b"fselect demo"),
        (0x30, b"if (0) 10 hidden Hidden"),
        (0x30, b"if (1) 20 left B"),
        (0x30, b"20 right C"),
    ]);
    input.localization = Some(Arc::new(cstl::Table {
        languages: vec!["en".into()],
        messages: ["first\x1fpart", "", "unused"]
            .map(|content| {
                vec![cstl::Message {
                    name: "".into(),
                    content: content.into(),
                }]
            })
            .to_vec(),
    }));
    let mut vm = cst::Interpreter::new(input, "en");
    let mut state = State::new(8);
    let mut source = Scenes(BTreeMap::from([(
        "right".into(),
        scene(&[(0x30, b"#2=9")]),
    )]));
    let Step::Event(cst::Event::Choices { choices, .. }) =
        vm.run(&mut state, &mut source, 20).unwrap()
    else {
        panic!()
    };
    assert_eq!(choices.iter().map(|c| c.id).collect::<Vec<_>>(), [20, 20]);
    assert_eq!(
        choices.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(),
        ["firstpart", "C"]
    );
    assert!(vm.resume().is_err());
    assert!(vm.choose(2, &mut source).is_err());
    vm.choose(1, &mut source).unwrap();
    assert_eq!(vm.run(&mut state, &mut source, 20).unwrap(), Step::Halted);
    assert_eq!(state.shared[2], 9);
}

#[test]
fn empty_choice_menu_continues_without_blocking_the_scene() {
    let input = scene(&[
        (0x30, b"fselect"),
        (0x30, b"if(0) 1 missing Hidden"),
        (0x30, b"#2=8"),
    ]);
    let mut vm = cst::Interpreter::new(input, "en");
    let mut state = State::new(8);
    let mut source = Scenes(BTreeMap::new());
    assert_eq!(vm.run(&mut state, &mut source, 20).unwrap(), Step::Halted);
    assert_eq!(state.shared[2], 8);
}

#[test]
fn cstl_ff_lengths_utf8_and_empty_segments() {
    fn ff(out: &mut Vec<u8>, mut n: usize) {
        while n >= 255 {
            out.push(255);
            n -= 255;
        }
        out.push(n as u8);
    }
    fn string(out: &mut Vec<u8>, s: &str) {
        ff(out, s.len());
        out.extend(s.as_bytes());
    }
    let mut bytes = b"CSTL\0\0\0\0".to_vec();
    ff(&mut bytes, 1);
    string(&mut bytes, "en");
    ff(&mut bytes, 2);
    string(&mut bytes, "");
    string(&mut bytes, &"字".repeat(170));
    string(&mut bytes, "N");
    string(&mut bytes, "first\x1fsecond\x1f");
    let table = cstl::Table::parse(&bytes).unwrap();
    assert_eq!(table.message("en", 0).unwrap().content.len(), 510);
    assert_eq!(table.segment("en", 1, 1), Some("second"));
    assert_eq!(table.segment("en", 1, 3), Some(""));
    assert_eq!(table.segment("missing", 1, 0), None);
    assert!(cstl::Table::parse(&bytes[..bytes.len() - 1]).is_err());
    bytes.push(0);
    assert!(cstl::Table::parse(&bytes).is_err());
}

#[test]
fn fes_control_flow_merges_blocks_and_keeps_local_state_per_instance() {
    let program = fes::Program::from_text(b"#START\n$4=1\ncall SUB\nif ($4==3)\n wait 2\nelse\n exit 9\nendif\nexit \\1\n#SUB\n$4+=1\n#SUB\n$4+=1\n\\1=7\nbreak\n").unwrap();
    assert!(program.diagnostics.is_empty());
    let mut vm = fes::Interpreter::new(program.clone()).unwrap();
    let mut state = State::new(8);
    state.local[1] = 99;
    assert_eq!(
        vm.run(&mut state, 100).unwrap(),
        Step::Event(fes::Event::Wait(2))
    );
    assert_eq!(state.shared[4], 3);
    assert_eq!(state.local[1], 99);
    vm.resume();
    assert_eq!(
        vm.run(&mut state, 100).unwrap(),
        Step::Event(fes::Event::Exit(7))
    );
    vm.resume();
    assert_eq!(
        vm.run(&mut state, 100).unwrap(),
        Step::Event(fes::Event::Frame)
    );
    let mut other =
        fes::Interpreter::new(fes::Program::from_text(b"#START\nexit \\1").unwrap()).unwrap();
    assert_eq!(
        other.run(&mut state, 10).unwrap(),
        Step::Event(fes::Event::Exit(0))
    );
}

#[test]
fn fes_preserves_mixed_text_columns_and_skips_orphan_terminators() {
    let text = "#OBJECT POS DISP\nlabel 0,0 1\n#START\nlabel text \"日本語//text\" // comment\nendif\nemit 7\n";
    let program = fes::Program::from_text(text.as_bytes()).unwrap();
    assert_eq!(program.declarations[0].header, b"OBJECT POS DISP");
    assert!(program.diagnostics.is_empty());
    let mut vm = fes::Interpreter::new(program).unwrap();
    let mut state = State::new(8);
    assert!(matches!(
        vm.run(&mut state, 20).unwrap(),
        Step::Event(fes::Event::InitializeObject(_))
    ));
    vm.resume();
    let Step::Event(fes::Event::Command(line)) = vm.run(&mut state, 20).unwrap() else {
        panic!()
    };
    assert_eq!(line.bytes, "label text \"日本語//text\"".as_bytes());
    vm.resume();
    let Step::Event(fes::Event::Command(line)) = vm.run(&mut state, 20).unwrap() else {
        panic!()
    };
    assert_eq!(line.bytes, b"emit 7");
    vm.resume();
    assert_eq!(
        vm.run(&mut state, 20).unwrap(),
        Step::Event(fes::Event::Frame)
    );
}

#[test]
fn fes_frame_restart_and_non_waiting_loop_have_distinct_yields() {
    let mut state = State::new(8);
    let mut vm = fes::Interpreter::new(fes::Program::from_text(b"#START\n$0+=1").unwrap()).unwrap();
    assert_eq!(
        vm.run(&mut state, 20).unwrap(),
        Step::Event(fes::Event::Frame)
    );
    assert_eq!(state.shared[0], 1);
    vm.resume();
    assert_eq!(
        vm.run(&mut state, 20).unwrap(),
        Step::Event(fes::Event::Frame)
    );
    assert_eq!(state.shared[0], 2);
    let mut looping =
        fes::Interpreter::new(fes::Program::from_text(b"#START\nnext START").unwrap()).unwrap();
    assert_eq!(looping.run(&mut state, 50).unwrap(), Step::Yield);
}

#[test]
fn anm_variable_loop_timed_events_and_bad_targets() {
    fn instruction(opcode: u32, values: &[u32]) -> anm::Instruction {
        let mut operands = [anm::Operand::default(); 8];
        for (i, &value) in values.iter().enumerate() {
            operands[i].value = value;
        }
        anm::Instruction { opcode, operands }
    }
    let program = anm::Program {
        instructions: vec![
            instruction(1, &[0, 1, 1]),
            instruction(0, &[10, 6, 6]),
            instruction(2, &[0, 1]),
        ],
    };
    let mut vm = anm::Interpreter::new(program);
    assert_eq!(
        vm.run(30).unwrap(),
        Step::Event(anm::Event::Image {
            id: 10,
            duration: 6
        })
    );
    assert_eq!(vm.run(30).unwrap(), Step::Halted);
    assert_eq!(vm.variables[0], 0);
    let mut looping = anm::Interpreter::new(anm::Program {
        instructions: vec![instruction(3, &[0])],
    });
    assert_eq!(looping.run(30).unwrap(), Step::Yield);
    let mut invalid = anm::Interpreter::new(anm::Program {
        instructions: vec![instruction(3, &[2])],
    });
    assert!(invalid.run(30).is_err());
}

fn ins(code: &mut Vec<u8>, opcode: u8, args: &[(u8, u32)]) {
    code.push(opcode);
    for &(flags, value) in args {
        code.push(flags);
        code.extend(value.to_le_bytes());
    }
}
struct Host {
    result: kcs::NativeReturn,
    calls: usize,
    arguments: Vec<u8>,
}
impl kcs::Host for Host {
    fn call(
        &mut self,
        index: u32,
        args: &[u8],
        _: &mut kcs::Interpreter,
    ) -> Result<kcs::NativeReturn> {
        assert_eq!(index, 7);
        self.calls += 1;
        self.arguments = args.to_vec();
        Ok(self.result)
    }
}

#[test]
fn kcs_native_statuses_and_retry_preserve_arguments() {
    for (result, status, has_value) in [
        (kcs::NativeReturn::Yield, 0, false),
        (kcs::NativeReturn::Value(42), 1, true),
        (kcs::NativeReturn::Continue, 2, false),
        (kcs::NativeReturn::YieldValue(42), 3, true),
        (kcs::NativeReturn::YieldVoid, 4, false),
        (kcs::NativeReturn::Retry, 5, false),
    ] {
        let mut code = Vec::new();
        ins(&mut code, 0x25, &[(3, 11)]);
        ins(&mut code, 0x25, &[(3, 22)]);
        ins(&mut code, 0x2b, &[(3, 7), (3, 8)]);
        ins(&mut code, 0x3b, &[]);
        let mut vm =
            kcs::Interpreter::new(kcs::Program::from_code(&code, vec![], 128).unwrap(), vec![])
                .unwrap();
        let mut host = Host {
            result,
            calls: 0,
            arguments: vec![],
        };
        let step = vm.run(&mut host, 50).unwrap();
        assert_eq!(host.calls, 1);
        assert_eq!(
            host.arguments,
            [11u32.to_le_bytes(), 22u32.to_le_bytes()].concat()
        );
        if status == 1 || status == 2 {
            assert_eq!(step, Step::Halted);
        } else {
            assert_eq!(
                step,
                Step::Event(kcs::Event {
                    native_index: 7,
                    status
                })
            );
        }
        if result == kcs::NativeReturn::Retry {
            assert_eq!(vm.stack_bytes(), host.arguments);
            host.result = kcs::NativeReturn::Value(42);
            assert_eq!(vm.run(&mut host, 50).unwrap(), Step::Halted);
            assert_eq!(host.calls, 2);
            assert_eq!(vm.stack_bytes(), 42u32.to_le_bytes());
        } else {
            assert_eq!(vm.stack_bytes().len(), if has_value { 4 } else { 0 });
        }
    }
}

#[test]
fn kcs_call_frame_return_and_typed_address_reading() {
    let mut code = Vec::new();
    ins(&mut code, 0x25, &[(3, 7)]);
    ins(&mut code, 0x2a, &[(3, 18), (3, 4)]);
    ins(&mut code, 0x3b, &[]);
    ins(&mut code, 0x31, &[(3, 4)]);
    ins(&mut code, 0x3d, &[(3, 0)]);
    ins(&mut code, 0x25, &[(0x73, 0)]);
    ins(&mut code, 0x25, &[(3, 5)]);
    ins(&mut code, 2, &[(0x43, 0), (0x43, 0)]);
    ins(&mut code, 0x2c, &[(3, 4), (3, 4)]);
    let mut vm =
        kcs::Interpreter::new(kcs::Program::from_code(&code, vec![], 128).unwrap(), vec![])
            .unwrap();
    let mut host = Host {
        result: kcs::NativeReturn::Continue,
        calls: 0,
        arguments: vec![],
    };
    assert_eq!(vm.run(&mut host, 100).unwrap(), Step::Halted);
    assert_eq!(vm.stack_bytes(), (-2i32).to_le_bytes());
    let mut code = Vec::new();
    ins(&mut code, 0x25, &[(0x11, 0)]);
    ins(&mut code, 0x25, &[(0x31, 0x80000000)]);
    ins(&mut code, 1, &[(0x31, 0x80000000), (3, 0)]);
    ins(&mut code, 0x3b, &[]);
    let mut vm = kcs::Interpreter::new(
        kcs::Program::from_code(&code, vec![255], 64).unwrap(),
        vec![255, 1, 2, 3],
    )
    .unwrap();
    vm.run(&mut host, 50).unwrap();
    assert_eq!(
        vm.stack_bytes(),
        [255u32.to_le_bytes(), [255, 1, 2, 3], u32::MAX.to_le_bytes()].concat()
    );
    assert!(vm.read(0xffff_ffff, 4).is_err());
}

#[test]
fn kcs_invalid_instruction_targets_and_stack_access_are_diagnostics() {
    assert!(kcs::Program::from_code(&[0x3f], vec![], 32).is_err());
    assert!(kcs::Program::from_code(&[0x25, 3, 0], vec![], 32).is_err());
    let mut code = Vec::new();
    ins(&mut code, 0x29, &[(3, 1)]);
    assert!(kcs::Program::from_code(&code, vec![], 32).is_err());
    let mut host = Host {
        result: kcs::NativeReturn::Continue,
        calls: 0,
        arguments: vec![],
    };
    let mut vm = kcs::Interpreter::new(
        kcs::Program::from_code(&[0x2d, 0x3b], vec![], 32).unwrap(),
        vec![],
    )
    .unwrap();
    assert!(vm.run(&mut host, 10).is_err());
    let mut code = Vec::new();
    ins(&mut code, 0x29, &[(3, 0)]);
    let mut vm =
        kcs::Interpreter::new(kcs::Program::from_code(&code, vec![], 32).unwrap(), vec![]).unwrap();
    assert_eq!(vm.run(&mut host, 50).unwrap(), Step::Yield);
}

#[test]
fn compressed_containers_initialize_data_and_reject_truncation() {
    let text = b"#START\nexit 0\n";
    let packed = compressed(text);
    let mut fes = b"FES\0".to_vec();
    word(&mut fes, packed.len());
    word(&mut fes, text.len());
    word(&mut fes, 0);
    fes.extend(packed);
    assert!(fes::Program::parse(&fes).is_ok());
    assert!(fes::Program::parse(&fes[..fes.len() - 1]).is_err());
    let mut init = Vec::new();
    word(&mut init, 8);
    word(&mut init, 1);
    init.extend(b"data\0");
    let code = [0x3b];
    let body = [code.as_slice(), init.as_slice()].concat();
    let mut header = Vec::new();
    for value in [
        0x0053_434b,
        2,
        0,
        52,
        1,
        53,
        1,
        init.len() as u32,
        8,
        32,
        53 + init.len() as u32,
        0,
        32,
    ] {
        header.extend(value.to_le_bytes());
    }
    header.extend(compressed(&body));
    let program = kcs::Program::parse(&header).unwrap();
    let vm = kcs::Interpreter::new(program, vec![]).unwrap();
    assert_eq!(vm.read(0, 8).unwrap(), b"\0data\0\0\0");
    assert!(kcs::Program::parse(&header[..header.len() - 1]).is_err());
}
