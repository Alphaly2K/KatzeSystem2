use katze_system2::{Step, expression::State, fes};

#[test]
fn fes_parameters_keep_tabs_groups_and_the_quote_termination_rule() {
    for (source, expected) in [
        ("cmd a b", vec!["cmd", "a", "b"]),
        ("cmd a\tb c", vec!["cmd", "a\tb", "c"]),
        ("cmd \"a b\" tail", vec!["cmd", "a b"]),
        ("cmd (4 + 5) tail", vec!["cmd", "(4 + 5)", "tail"]),
        ("cmd (:a b)tail end", vec!["cmd", "a btail", "end"]),
        ("cmd 'a b' tail", vec!["cmd", "'a", "b'", "tail"]),
        ("cmd a(b)c(d) tail", vec!["cmd", "a", "(b)c", "(d)", "tail"]),
        ("break 4 + 5", vec!["break", "4", "+", "5"]),
        ("cmd (:) tail", vec!["cmd"]),
        ("cmd 1(2) tail", vec!["cmd", "1(2)", "tail"]),
        ("cmd \"x\\\"y\" last", vec!["cmd", "x\\"]),
        (
            "cmd (1 + (2 * 3))x tail",
            vec!["cmd", "(1 + (2 * 3))x", "tail"],
        ),
    ] {
        assert_eq!(
            fes::parameters(source.as_bytes()).unwrap(),
            expected.into_iter().map(str::as_bytes).collect::<Vec<_>>(),
            "{source}"
        );
    }
    let source = b"#START\nwait \"3\" ignored\nbreak 4 + 5\nexit 99\n";
    let mut vm = fes::Interpreter::new(fes::Program::from_text(source).unwrap()).unwrap();
    let mut state = State::new(8);
    assert_eq!(
        vm.run(&mut state, 30).unwrap(),
        Step::Event(fes::Event::Wait(3))
    );
    vm.resume();
    assert_eq!(
        vm.run(&mut state, 30).unwrap(),
        Step::Event(fes::Event::Frame)
    );
    assert_eq!(vm.return_value, 4);
}

#[test]
fn macros_replace_whole_tokens_once_and_preserve_quoted_bytes() {
    let program = fes::Program::from_text(
        b"#MACRO\nCOUNTER 7\nA B\nB 9\nNAME object\nwait 23\nTEXT \"a b\"\n#START\nexit 0\n",
    )
    .unwrap();
    for (input, expected) in [
        ("\\COUNTER=1", "\\7=1"),
        ("A B A1 A_B", "B 9 A1 A_B"),
        ("NAME[COUNTER] load NAME.png", "object[7] load object.png"),
        ("\"COUNTER NAME\" NAME", "\"COUNTER NAME\" object"),
        ("'COUNTER' COUNTER", "'COUNTER' 7"),
        ("wait COUNTER", "23 7"),
        (
            "123COUNTER COUNTER123 0xCOUNTER",
            "1237 COUNTER123 0xCOUNTER",
        ),
        ("TEXT", "a b"),
        ("\"x\\\"COUNTER\" NAME", "\"x\\\"COUNTER\" object"),
    ] {
        assert_eq!(
            program.expand_macros(input.as_bytes()).unwrap(),
            expected.as_bytes()
        );
    }
    assert_eq!(
        program.expand_macros(b"\x82\x5cNAME NAME").unwrap(),
        b"\x82\x5cobject object"
    );
    assert_eq!(
        program.expand_macros(b"\"\x82\x5c\" NAME").unwrap(),
        b"\"\x82\x5c\" object"
    );
}

#[test]
fn repeated_macro_declarations_expand_runtime_strings_before_commands() {
    let program = fes::Program::from_text(
        b"#MACRO\nCOUNTER 7\nCOND if\n#MACRO\nCOUNTER 8\n#START\n\\COUNTER=3\nCOND (\\COUNTER==3)\n emit $str2 \"COUNTER\" 'a//b'\nelse\n exit 99\nendif\nexit 0\n"
    ).unwrap();
    assert_eq!(program.declarations.len(), 2);
    let mut state = State::new(16);
    state.local[8] = 99;
    state.strings.insert(2, b"COUNTER".to_vec());
    let mut vm = fes::Interpreter::new(program).unwrap();
    let event = vm.run(&mut state, 30).unwrap();
    let Step::Event(fes::Event::Command(line)) = &event else {
        panic!("{event:?}");
    };
    assert_eq!(line.bytes, b"emit 8 \"COUNTER\" 'a//b'");
    assert_eq!(vm.run(&mut state, 30).unwrap(), event);
    assert_eq!(state.local[8], 99);
    vm.resume();
    assert_eq!(
        vm.run(&mut state, 30).unwrap(),
        Step::Event(fes::Event::Exit(0))
    );
}

#[test]
fn missing_optional_calls_return_to_nested_and_event_callers() {
    let source = b"#START\ncall OUTER\n$0+=1\nwait 9\n#OUTER\ncall OPTIONAL\n$1+=1\n#button.PUSH_L\ncall OPTIONAL\n$2+=1\n";
    for budget in [1, 128] {
        let mut vm = fes::Interpreter::new(fes::Program::from_text(source).unwrap()).unwrap();
        let mut state = State::new(4);
        loop {
            match vm.run(&mut state, budget).unwrap() {
                Step::Yield => (),
                event => {
                    assert_eq!(event, Step::Event(fes::Event::Wait(9)));
                    break;
                }
            }
        }
        assert_eq!(&state.shared[..3], &[1, 1, 0]);
        assert!(vm.dispatch_event("button", 0, "PUSH_L", None).unwrap());
        for _ in 0..20 {
            assert_eq!(vm.run(&mut state, budget).unwrap(), Step::Yield);
            if vm.event_context().is_none() {
                break;
            }
        }
        assert!(vm.event_context().is_none());
        assert_eq!(&state.shared[..3], &[1, 1, 1]);
        assert_eq!(
            vm.run(&mut state, 128).unwrap(),
            Step::Event(fes::Event::Wait(9))
        );
        let source =
            b"#START\nwait 9\n#DEFAULT.PUSH_L\ncall OPTIONAL\n$0+=1\n#button.PUSH_L\n$1+=1\n";
        let mut vm = fes::Interpreter::new(fes::Program::from_text(source).unwrap()).unwrap();
        let mut state = State::new(4);
        assert_eq!(
            vm.run(&mut state, 128).unwrap(),
            Step::Event(fes::Event::Wait(9))
        );
        assert!(
            vm.dispatch_event("button", 0, "PUSH_L", Some("DEFAULT"))
                .unwrap()
        );
        for _ in 0..20 {
            assert_eq!(vm.run(&mut state, budget).unwrap(), Step::Yield);
            if vm.event_context().is_none() {
                break;
            }
        }
        assert!(vm.event_context().is_none());
        assert_eq!(&state.shared[..2], &[0, 1]);
        assert_eq!(
            vm.run(&mut state, 128).unwrap(),
            Step::Event(fes::Event::Frame)
        );
        assert_eq!(&state.shared[..2], &[1, 1]);
    }
}

#[test]
fn next_discards_all_subroutine_return_locations() {
    let program = fes::Program::from_text(
        b"#START\ncall OUTER\nexit 99\n#OUTER\ncall INNER\nexit 98\n#INNER\nnext FINAL\n#FINAL\nexit 7\n"
    ).unwrap();
    let mut vm = fes::Interpreter::new(program).unwrap();
    assert_eq!(
        vm.run(&mut State::new(8), 30).unwrap(),
        Step::Event(fes::Event::Exit(7))
    );
}

#[test]
fn acknowledged_exit_continues_the_subroutine_and_its_caller_in_the_same_frame() {
    let program = fes::Program::from_text(
        b"#START\n$0=$0+1\ncall SUB\n$1=$1+1\nwait 5\n#SUB\n$2=$2+1\nexit 37\n$1=999\n",
    )
    .unwrap();
    let mut state = State::new(4);
    let mut vm = fes::Interpreter::new(program).unwrap();
    assert_eq!(
        vm.run(&mut state, 128).unwrap(),
        Step::Event(fes::Event::Exit(37))
    );
    assert_eq!(&state.shared[..3], &[1, 0, 1]);
    assert_eq!(
        vm.run(&mut state, 128).unwrap(),
        Step::Event(fes::Event::Exit(37))
    );
    vm.resume();
    assert_eq!(
        vm.run(&mut state, 128).unwrap(),
        Step::Event(fes::Event::Wait(5))
    );
    assert_eq!(&state.shared[..3], &[1, 1000, 1]);
}

#[test]
fn break_ends_the_current_block_and_keeps_its_value_across_returns() {
    let program = fes::Program::from_text(
        b"#START\ncall OUTER\nemit done\nbreak\nexit 99\n#OUTER\ncall INNER\nemit returned\nbreak (4 + 5)\nexit 98\n#INNER\nbreak -1\nexit 97\n",
    )
    .unwrap();
    let mut vm = fes::Interpreter::new(program).unwrap();
    let mut state = State::new(8);
    for (expected, value) in [(b"emit returned".as_slice(), -1), (b"emit done", 9)] {
        let Step::Event(fes::Event::Command(line)) = vm.run(&mut state, 30).unwrap() else {
            panic!("expected command");
        };
        assert_eq!(line.bytes, expected);
        assert_eq!(vm.return_value, value);
        vm.resume();
    }
    assert_eq!(
        vm.run(&mut state, 30).unwrap(),
        Step::Event(fes::Event::Frame)
    );
    assert_eq!(vm.return_value, 0);
}

#[test]
fn object_events_preserve_the_baseline_default_and_own_transfer_rules() {
    let cases = [
        (
            "#DEFAULT.PUSH_L\n$0=$0*10+1\n#button.PUSH_L\n$0=$0*10+2\n",
            Some("DEFAULT"),
            12,
            None,
        ),
        (
            "#DEFAULT.PUSH_L\n$0=$0*10+1\nnext DEST\n#button.PUSH_L\n$0=$0*10+2\n#DEST\n$0=$0*10+3\n",
            Some("DEFAULT"),
            132,
            None,
        ),
        (
            "#DEFAULT.PUSH_L\n$0=$0*10+1\n#button.PUSH_L\n$0=$0*10+2\nnext DEST\n$0=999\n#DEST\n$0=$0*10+3\n",
            Some("DEFAULT"),
            12,
            Some(123),
        ),
        (
            "#DEFAULT.PUSH_L\ncall SUB\n$0=$0*10+1\n#button.PUSH_L\n$0=$0*10+2\n#SUB\n$0=$0*10+3\n",
            Some("DEFAULT"),
            32,
            Some(321),
        ),
        (
            "#button.PUSH_L\ncall SUB\n$0=$0*10+2\n#SUB\n$0=$0*10+3\n",
            None,
            32,
            None,
        ),
    ];
    for (source, default, expected, transferred) in cases {
        let source = format!("#START\nwait 77\n$1=1\nbreak\n{source}");
        let mut vm =
            fes::Interpreter::new(fes::Program::from_text(source.as_bytes()).unwrap()).unwrap();
        let mut state = State::new(8);
        assert_eq!(
            vm.run(&mut state, 30).unwrap(),
            Step::Event(fes::Event::Wait(77))
        );
        assert!(vm.dispatch_event("button", 7, "PUSH_L", default).unwrap());
        assert_eq!(vm.event_context().unwrap().object, "button");
        assert!(vm.dispatch_event("button", 0, "PUSH_L", None).is_err());
        for _ in 0..100 {
            assert_eq!(vm.run(&mut state, 1).unwrap(), Step::Yield);
            if vm.event_context().is_none() {
                break;
            }
        }
        assert!(vm.event_context().is_none(), "{source}");
        assert_eq!(state.shared[0], expected, "{source}");
        assert_eq!(state.current, 0);
        if let Some(value) = transferred {
            assert_eq!(
                vm.run(&mut state, 30).unwrap(),
                Step::Event(fes::Event::Frame)
            );
            assert_eq!(state.shared[0], value);
            assert_eq!(state.shared[1], 0);
        } else {
            assert_eq!(
                vm.run(&mut state, 30).unwrap(),
                Step::Event(fes::Event::Wait(77))
            );
            vm.resume();
            assert_eq!(
                vm.run(&mut state, 30).unwrap(),
                Step::Event(fes::Event::Frame)
            );
            assert_eq!(state.shared[1], 1);
        }
    }
}

#[test]
fn event_commands_keep_context_and_event_next_observes_the_initial_call_depth() {
    let source = b"#START\ncall MAIN\nexit 99\n#MAIN\nwait 77\n$2=@\nemit original\n#button.PUSH_L\n$0=@\n@ setid 2\nnext DEST\n#DEST\n$0+=1\n";
    let mut vm = fes::Interpreter::new(fes::Program::from_text(source).unwrap()).unwrap();
    let mut state = State::new(8);
    state.current = 11;
    assert_eq!(
        vm.run(&mut state, 30).unwrap(),
        Step::Event(fes::Event::Wait(77))
    );
    assert!(!vm.dispatch_event("missing", 3, "PUSH_L", None).unwrap());
    assert!(vm.dispatch_event("button", 3, "PUSH_L", None).unwrap());
    let event = vm.run(&mut state, 30).unwrap();
    let Step::Event(fes::Event::Command(line)) = &event else {
        panic!("{event:?}");
    };
    assert_eq!(line.bytes, b"@ setid 2");
    assert_eq!(vm.event_context().unwrap().bank, 3);
    assert_eq!(vm.run(&mut state, 30).unwrap(), event);
    vm.resume();
    assert_eq!(vm.run(&mut state, 30).unwrap(), Step::Yield);
    assert!(vm.event_context().is_none());
    assert_eq!(state.shared[0], 4);
    assert_eq!(
        vm.run(&mut state, 30).unwrap(),
        Step::Event(fes::Event::Wait(77))
    );
    vm.resume();
    let Step::Event(fes::Event::Command(line)) = vm.run(&mut state, 30).unwrap() else {
        panic!("expected original main block");
    };
    assert_eq!(line.bytes, b"emit original");
    assert_eq!(state.shared[2], 11);
    vm.resume();
    assert_eq!(
        vm.run(&mut state, 30).unwrap(),
        Step::Event(fes::Event::Frame)
    );
}

#[test]
fn event_wait_and_exit_end_one_handler_and_preserve_the_main_wait() {
    for terminator in ["wait 5", "exit 9"] {
        for default in [false, true] {
            let handlers = if default {
                format!("#DEFAULT.PUSH_L\n$0=1\n{terminator}\n$0=99\n#button.PUSH_L\n$0=$0*10+2\n")
            } else {
                format!("#button.PUSH_L\n$0=1\n{terminator}\n$0=99\n")
            };
            let source = format!("#START\nwait 77\n$1=1\n{handlers}");
            let mut vm =
                fes::Interpreter::new(fes::Program::from_text(source.as_bytes()).unwrap()).unwrap();
            let mut state = State::new(8);
            assert_eq!(
                vm.run(&mut state, 30).unwrap(),
                Step::Event(fes::Event::Wait(77))
            );
            assert!(
                vm.dispatch_event("button", 7, "PUSH_L", default.then_some("DEFAULT"))
                    .unwrap()
            );
            for _ in 0..30 {
                match vm.run(&mut state, 1).unwrap() {
                    Step::Yield => (),
                    Step::Event(fes::Event::Wait(5) | fes::Event::Exit(9)) => {
                        vm.finish_event_handler().unwrap();
                    }
                    other => panic!("{other:?}"),
                }
                if vm.event_context().is_none() {
                    break;
                }
            }
            assert!(vm.event_context().is_none());
            assert_eq!(state.shared[0], if default { 12 } else { 1 });
            assert_eq!(
                vm.run(&mut state, 30).unwrap(),
                Step::Event(fes::Event::Wait(77))
            );
            assert_eq!(state.shared[1], 0);
            assert!(vm.finish_event_handler().is_err());
        }
    }
}

#[test]
fn definitions_create_banks_before_initialization_and_do_not_expand_macros() {
    let source = b"#MACRO\nTARGET renamed\n#OBJECT POS\nTARGET 0,0\n#DEFINE\nIMAGE TARGET[($0+=1)]\nbutton choices[2]\nIMAGE zero[0]\nIMAGE expr[(1+2)]\nIMAGE TARGET[2]\n#START\nexit 0\n";
    let mut vm = fes::Interpreter::new(fes::Program::from_text(source).unwrap()).unwrap();
    let mut state = State::new(8);
    assert!(vm.dispatch_event("TARGET", 0, "FOCUS", None).is_err());
    for (kind, name, banks) in [
        ("IMAGE", "TARGET", 1),
        ("BUTTON", "choices", 2),
        ("IMAGE", "zero", 1),
        ("IMAGE", "expr", 3),
        ("IMAGE", "TARGET", 2),
    ] {
        let event = Step::Event(fes::Event::CreateObject(fes::ObjectDefinition {
            kind,
            name: name.to_owned(),
            banks,
        }));
        assert_eq!(vm.run(&mut state, 1).unwrap(), event);
        assert_eq!(vm.run(&mut state, 1).unwrap(), event);
        assert_eq!(state.shared[0], 1);
        vm.resume();
    }
    let Step::Event(fes::Event::InitializeObject(initialization)) = vm.run(&mut state, 1).unwrap()
    else {
        panic!("expected object initialization");
    };
    assert_eq!(initialization.target.name, "TARGET");
    assert_eq!(initialization.target.bank, -1);
    assert_eq!(initialization.attributes.position, [Some(0), Some(0), None]);
    vm.resume();
    assert_eq!(
        vm.run(&mut state, 10).unwrap(),
        Step::Event(fes::Event::Exit(0))
    );
}

#[test]
fn definitions_require_explicit_array_redefinitions_and_valid_counts() {
    let source = b"#DEFINE\nIMAGE array[2]\nIMAGE array\n#START\nexit 0\n";
    let mut vm = fes::Interpreter::new(fes::Program::from_text(source).unwrap()).unwrap();
    let mut state = State::new(8);
    assert!(matches!(
        vm.run(&mut state, 1).unwrap(),
        Step::Event(fes::Event::CreateObject(_))
    ));
    vm.resume();
    assert!(vm.run(&mut state, 1).is_err());
    for declaration in ["K target[2]", "IMAGE target[-2]", "IMAGE target[]"] {
        let source = format!("#MACRO\nK IMAGE\n#DEFINE\n{declaration}\n#START\nexit 0\n");
        let mut vm =
            fes::Interpreter::new(fes::Program::from_text(source.as_bytes()).unwrap()).unwrap();
        assert!(vm.run(&mut State::new(8), 1).is_err(), "{declaration}");
    }
}

#[test]
fn initialization_preserves_partial_values_and_runs_between_declaration_stages() {
    let source = b"#MACRO\nPOS ignored\n#DEFINE\nIMAGE sprite[2]\nIMAGE anchor[2]\nVIEWPORT viewport\n#KEYBLOCK\n2\n#OBJECT FILE ID.N ID.P POS BASE BASE SIZE PRI DISP ENABLE KEY_U VIEWPORT POS2\nsprite $str9 0 3 (1+2),,7 1,2,3 9 80 ($4+=1) 0 1 0 viewport (:@+20%,@)\n#OBJECT ID.32 POS.1\nsprite[1] 6,7 anchor[1]\n#SUBCOMMAND\noneshot sample item : [1]\n#START\n$4=123\nexit 0\n";
    let mut vm = fes::Interpreter::new(fes::Program::from_text(source).unwrap()).unwrap();
    let mut state = State::new(16);
    state.shared[4] = 6;
    state.strings.insert(9, b"texture\x82\xa0".to_vec());
    let Step::Event(fes::Event::Declarations(declarations)) = vm.run(&mut state, 20).unwrap()
    else {
        panic!("expected keyblock declaration");
    };
    assert_eq!(declarations[0].header, b"KEYBLOCK");
    vm.resume();
    for _ in 0..3 {
        assert!(matches!(
            vm.run(&mut state, 20).unwrap(),
            Step::Event(fes::Event::CreateObject(_))
        ));
        vm.resume();
    }
    let event = vm.run(&mut state, 20).unwrap();
    let Step::Event(fes::Event::InitializeObject(initialization)) = &event else {
        panic!("{event:?}");
    };
    assert_eq!(
        initialization.target,
        fes::ObjectReference {
            name: "sprite".into(),
            bank: -1
        }
    );
    let a = &initialization.attributes;
    assert_eq!(a.file.as_deref(), Some(b"texture\x82\xa0".as_slice()));
    assert_eq!(a.frames[0], Some(0));
    assert_eq!(a.frames[3], Some(3));
    assert_eq!(a.frames[1], None);
    assert_eq!(a.position, [Some(3), Some(0), Some(7)]);
    assert_eq!(a.base, Some([9, 2, 3]));
    assert_eq!(a.size, Some([80, 0]));
    assert_eq!(a.priority, Some(7));
    assert!(a.sequential_priority);
    assert_eq!(a.display, Some(0));
    assert_eq!(a.enabled, Some(1));
    assert_eq!(a.blend, None);
    assert_eq!(
        a.keys[0],
        Some(fes::ObjectReference {
            name: String::new(),
            bank: 0
        })
    );
    assert_eq!(
        a.viewport,
        Some(fes::ObjectReference {
            name: "viewport".into(),
            bank: 0
        })
    );
    assert_eq!(a.position_resource.as_deref(), Some(b"@+20%,@".as_slice()));
    assert_eq!(vm.run(&mut state, 20).unwrap(), event);
    assert_eq!(state.shared[4], 7);
    assert!(vm.dispatch_event("sprite", 0, "FOCUS", None).is_err());
    vm.resume();
    let Step::Event(fes::Event::InitializeObject(initialization)) = vm.run(&mut state, 20).unwrap()
    else {
        panic!("expected second object block");
    };
    assert_eq!(initialization.target.bank, 1);
    assert_eq!(initialization.attributes.position, [Some(6), Some(7), None]);
    assert_eq!(
        initialization.attributes.plane,
        Some(fes::ObjectReference {
            name: "anchor".into(),
            bank: 1
        })
    );
    vm.resume();
    let Step::Event(fes::Event::Declarations(declarations)) = vm.run(&mut state, 20).unwrap()
    else {
        panic!("expected subcommand declaration");
    };
    assert_eq!(declarations[0].header, b"SUBCOMMAND");
    vm.resume();
    assert_eq!(
        vm.run(&mut state, 20).unwrap(),
        Step::Event(fes::Event::Exit(0))
    );
    assert_eq!(state.shared[4], 123);
}

#[test]
fn named_bindings_are_scoped_to_the_fes_and_reach_initializers_and_conditions() {
    let source = b"#DEFINE\nIMAGE sprite[COUNT]\n#OBJECT POS\nsprite _X_,value[1]\n#START\nif (sprite[1]!=0&&_FRAME_==5)\n$4=value[($4+=1)]\nendif\nexit $4\n";
    let mut vm = fes::Interpreter::new(fes::Program::from_text(source).unwrap()).unwrap();
    for (name, bank, value) in [
        ("COUNT", 0, 2),
        ("_X_", 0, 17),
        ("value", 1, 42),
        ("_FRAME_", 0, 5),
    ] {
        vm.named.entry(name.into()).or_default().insert(bank, value);
    }
    let mut state = State::new(8);
    state.named.entry("_X_".into()).or_default().insert(0, 999);
    let Step::Event(fes::Event::CreateObject(definition)) = vm.run(&mut state, 20).unwrap() else {
        panic!("expected object creation");
    };
    assert_eq!(definition.banks, 2);
    vm.named.entry("sprite".into()).or_default().insert(1, 77);
    vm.resume();
    let Step::Event(fes::Event::InitializeObject(initialization)) = vm.run(&mut state, 20).unwrap()
    else {
        panic!("expected object initialization");
    };
    assert_eq!(
        initialization.attributes.position,
        [Some(17), Some(42), None]
    );
    vm.resume();
    assert_eq!(
        vm.run(&mut state, 20).unwrap(),
        Step::Event(fes::Event::Exit(42))
    );
    assert_eq!(state.named["_X_"][&0], 999);
    assert!(!state.named.contains_key("sprite"));
    assert_eq!(vm.named["sprite"][&1], 77);
}

#[test]
fn subcommands_preserve_defaults_mappings_and_first_definition_across_sections() {
    let source = b"#SUBCOMMAND\noneshot move motion p[($4+=1)] px PX[20%] : [1] [0x2tail] -5 0x20 7tail cxr CA\n#SUBCOMMAND\ncontinue move other p[9] : [0]\ncontinue repeat other p : [0]\n#START\nexit 0\n";
    let vm = fes::Interpreter::new(fes::Program::from_text(source).unwrap()).unwrap();
    let mut state = State::new(8);
    state.shared[4] = 7;
    let table = vm.subcommands(&state);
    assert!(table.diagnostics.is_empty());
    assert_eq!(table.bindings.len(), 2);
    let binding = &table.bindings[b"move".as_slice()];
    assert_eq!(binding.mode, fes::SubcommandMode::Oneshot);
    assert_eq!(binding.target, b"motion");
    assert_eq!(binding.parameters[0].descriptor, "p");
    assert_eq!(binding.parameters[0].default, b"($4+=1)");
    assert_eq!(binding.parameters[1].descriptor, "px");
    assert_eq!(binding.parameters[1].default, b"-1");
    assert_eq!(binding.parameters[2].descriptor, "PX");
    assert_eq!(binding.parameters[2].default, b"20%");
    assert_eq!(
        binding.arguments,
        vec![
            fes::MappedArgument::Parameter(1),
            fes::MappedArgument::Parameter(2),
            fes::MappedArgument::Integer(-5),
            fes::MappedArgument::Integer(32),
            fes::MappedArgument::Integer(7),
            fes::MappedArgument::Context("cxr"),
            fes::MappedArgument::Context("CA"),
        ]
    );
    assert_eq!(state.shared[4], 7);
    assert_eq!(
        table.bindings[b"repeat".as_slice()].mode,
        fes::SubcommandMode::Continue
    );
}

#[test]
fn malformed_subcommands_report_offsets_and_allow_following_bindings() {
    let source = b"#SUBCOMMAND\nbare\nONESHOT a motion : [0]\noneshot b motion pxx[1] : [0]\noneshot c motion p[1]\noneshot d motion p[1234567890123456] : [0]\noneshot e motion p : unknown\noneshot good motion p : [0]\n#START\nexit 0\n";
    let vm = fes::Interpreter::new(fes::Program::from_text(source).unwrap()).unwrap();
    let table = vm.subcommands(&State::new(8));
    assert_eq!(table.diagnostics.len(), 6);
    assert_eq!(table.diagnostics[0].offset, b"#SUBCOMMAND\n".len());
    assert_eq!(table.bindings.len(), 1);
    assert!(table.bindings.contains_key(b"good".as_slice()));
}

#[test]
fn keyblocks_keep_defaults_partial_neighbors_and_first_section() {
    let source = b"#KEYBLOCK 0x3tail\n0 4,5 1,2,3,4\n0 9,8 7\n1 ,7 ,2,,4\n2 6,7\n99 10,10 0,0,0,0\n-1 9,9\n#KEYBLOCK 4\n2 99,99 0,0,0,0\n#START\nexit 0\n";
    let mut vm = fes::Interpreter::new(fes::Program::from_text(source).unwrap()).unwrap();
    let blocks = vm.keyblocks(&mut State::new(8)).unwrap();
    assert_eq!(
        blocks,
        vec![
            fes::KeyBlock {
                size: [9, 8],
                neighbors: [7, 2, 3, 4]
            },
            fes::KeyBlock {
                size: [0, 7],
                neighbors: [0, 2, 0, 4]
            },
            fes::KeyBlock {
                size: [6, 7],
                neighbors: [-1; 4]
            },
        ]
    );
    let source = b"#KEYBLOCK 2\n($4=1) ($4+=1),($4+=1) ($4+=1),($4+=1)\n#START\nexit 0\n";
    let mut vm = fes::Interpreter::new(fes::Program::from_text(source).unwrap()).unwrap();
    let mut state = State::new(8);
    let blocks = vm.keyblocks(&mut state).unwrap();
    assert_eq!(blocks[0], fes::KeyBlock::default());
    assert_eq!(
        blocks[1],
        fes::KeyBlock {
            size: [2, 3],
            neighbors: [4, 5, -1, -1]
        }
    );
    assert_eq!(state.shared[4], 5);
}

#[test]
fn keyblock_invalid_counts_and_dimensions_are_diagnostics() {
    for declaration in ["#KEYBLOCK (1+2)", "#KEYBLOCK 1000001", "#KEYBLOCK 2\n0 4"] {
        let source = format!("{declaration}\n#START\nexit 0\n");
        let mut vm =
            fes::Interpreter::new(fes::Program::from_text(source.as_bytes()).unwrap()).unwrap();
        assert!(vm.keyblocks(&mut State::new(8)).is_err(), "{declaration}");
    }
}

#[test]
fn image_file_and_frame_initialization_preserve_type_and_update_order() {
    let mut image = fes::ImageState::default();
    let mut attributes = fes::ObjectAttributes {
        file: Some(b"texture".to_vec()),
        image_number: Some(50),
        size: Some([123, 45]),
        color: Some(7),
        path_mode: Some(2),
        ..Default::default()
    };
    attributes.frames[1] = Some(77);
    image
        .apply(&attributes, 0, |name| {
            Ok([b"prefix/".as_slice(), name].concat())
        })
        .unwrap();
    assert_eq!(image.requested_file, b"texture");
    assert_eq!(image.file, b"prefix/texture");
    assert_eq!(image.image_number, 50);
    assert_eq!(image.selected_frame(), 0);
    assert_eq!(image.base.size, [-1; 2]);
    assert_eq!(image.base.color, -1);
    assert_eq!(image.path_mode, 2);
    attributes.file = Some(Vec::new());
    attributes.image_number = Some(1);
    attributes.frames = [None; 32];
    image
        .apply(&attributes, 0, |_| {
            panic!("empty name must not be transformed")
        })
        .unwrap();
    assert_eq!(image.requested_file, b"");
    assert_eq!(image.file, b"prefix/texture");
    assert_eq!(image.selected_frame(), 77);
    attributes.image_number = Some(50);
    image.apply(&attributes, 0, |_| unreachable!()).unwrap();
    assert_eq!(image.image_number, 1);
    assert_eq!(image.selected_frame(), 77);
}

#[test]
fn image_frame_mapping_changes_wait_for_idn_and_repeated_file_names_skip_reload() {
    let mut image = fes::ImageState::default();
    let id = fes::ObjectAttributes {
        image_number: Some(0),
        ..Default::default()
    };
    assert!(!image.base.updates.needs_rebuild());
    image.apply(&id, 0, |_| unreachable!()).unwrap();
    assert!(image.base.updates.needs_rebuild());
    assert!(!image.frame_mapping_changed);
    image.base.updates = Default::default();
    image.apply(&id, 0, |_| unreachable!()).unwrap();
    assert!(!image.base.updates.needs_rebuild());
    let file = fes::ObjectAttributes {
        file: Some(b"texture".to_vec()),
        ..Default::default()
    };
    image.apply(&file, 0, |v| Ok(v.to_vec())).unwrap();
    assert!(image.base.updates.needs_rebuild());
    image.base.updates = Default::default();
    image.apply(&file, 0, |v| Ok(v.to_vec())).unwrap();
    assert!(!image.base.updates.needs_rebuild());
    let mut mapping = fes::ObjectAttributes::default();
    mapping.frames[0] = Some(17);
    image.apply(&mapping, 0, |_| unreachable!()).unwrap();
    assert_eq!(image.selected_frame(), 17);
    assert!(image.frame_mapping_changed);
    assert!(!image.base.updates.needs_rebuild());
    image
        .apply(
            &fes::ObjectAttributes {
                image_number: Some(99),
                ..Default::default()
            },
            0,
            |_| unreachable!(),
        )
        .unwrap();
    assert!(image.frame_mapping_changed);
    assert!(!image.base.updates.needs_rebuild());
    image.apply(&id, 0, |_| unreachable!()).unwrap();
    assert!(!image.frame_mapping_changed);
    assert!(image.base.updates.needs_rebuild());
    assert!(!image.base.updates.is_dirty());
}

#[test]
fn cimage_applies_its_columns_with_float_rounding_and_baseline_priority_updates() {
    let mut image = fes::CImageState {
        position: [-7.5, 8.75],
        ..Default::default()
    };
    let attributes = fes::ObjectAttributes {
        position: [Some(12), None, Some(999)],
        size: Some([33, 44]),
        color: Some(-99),
        blend: Some(500),
        display: Some(-3),
        mask: Some(5),
        button_type: Some(8),
        priority: Some(-2),
        sequential_priority: true,
        parent_priority: Some(999),
        plane: Some(fes::ObjectReference {
            name: "plane".into(),
            bank: 2,
        }),
        file: Some(b"ignored".to_vec()),
        ..Default::default()
    };
    image.apply(&attributes, 4);
    assert_eq!(image.position, [12.0, 8.0]);
    assert_eq!(image.size, [33, 44]);
    assert_eq!(image.color, -99);
    assert_eq!(image.blend, 500);
    assert_eq!(image.display, -3);
    assert_eq!(image.mask, 5);
    assert_eq!(image.button_type, 8);
    assert_eq!(image.plane, attributes.plane.unwrap());
    assert_eq!(image.priority, 22);
    assert_eq!(image.parent_priority, 2);
    image.apply(
        &fes::ObjectAttributes {
            priority: Some(22),
            ..Default::default()
        },
        0,
    );
    assert_eq!(image.priority, 22);
    assert_eq!(image.parent_priority, 22);
    image.apply(
        &fes::ObjectAttributes {
            position: [Some(i32::MAX), None, None],
            ..Default::default()
        },
        0,
    );
    assert_eq!(image.position[0].to_bits(), (i32::MAX as f32).to_bits());
    image.apply(
        &fes::ObjectAttributes {
            position: [None, Some(0), None],
            ..Default::default()
        },
        0,
    );
    assert_eq!(image.position, [i32::MIN as f32, 0.0]);
}

#[test]
fn cimage_keeps_source_property_and_priority_updates_independent() {
    let mut image = fes::CImageState::default();
    assert!(!image.updates.is_dirty());
    assert!(!image.updates.needs_rebuild());
    assert!(!image.updates.priority_changed());
    image.apply(
        &fes::ObjectAttributes {
            size_resource: Some(b"dimensions:7".to_vec()),
            priority: Some(4),
            sequential_priority: true,
            ..Default::default()
        },
        2,
    );
    assert!(!image.updates.is_dirty());
    assert!(!image.updates.needs_rebuild());
    assert!(image.updates.priority_changed());
    assert_eq!((image.priority, image.parent_priority), (26, 6));
    image.updates = Default::default();
    image.apply(
        &fes::ObjectAttributes {
            size: Some([-1; 2]),
            display: Some(0),
            ..Default::default()
        },
        0,
    );
    assert!(!image.updates.is_dirty());
    assert!(!image.updates.needs_rebuild());
    for _ in 0..2 {
        image.updates = Default::default();
        image.apply(
            &fes::ObjectAttributes {
                color: Some(-1),
                ..Default::default()
            },
            0,
        );
        assert!(image.updates.needs_rebuild());
        assert!(!image.updates.is_dirty());
    }
    image.updates = Default::default();
    let mut properties = image.motion_properties();
    properties[6] = 1.25f32.to_bits();
    properties[5] = 10f32.to_bits();
    assert!(image.apply_motion_properties(&properties));
    assert!(image.updates.is_dirty());
    assert!(!image.updates.needs_rebuild());
    assert!(image.updates.priority_changed());
    assert_eq!((image.priority, image.parent_priority), (30, 6));
}

#[test]
fn cimage_key_position_keeps_cached_offsets_across_modes_and_same_size_updates() {
    let mut image = fes::CImageState::default();
    assert_eq!(image.key_position, [0; 2]);
    image.set_size([2; 2]);
    assert_eq!(image.key_position, [-14, -6]);
    image.updates = Default::default();
    image.set_key_position(0, [99; 2]);
    image.set_key_position(0, [99; 2]);
    assert_eq!(image.key_position, [-12, -4]);
    image.set_size([2; 2]);
    assert_eq!(image.key_position, [-14, -6]);
    assert!(!image.updates.needs_rebuild());
    assert!(!image.updates.is_dirty());

    image.set_key_position(-3, [i32::MAX, i32::MIN]);
    image.set_size([33, -3]);
    assert_eq!(image.key_position_mode, -3);
    assert_eq!(image.key_position, [i32::MAX, i32::MIN]);
    image.updates = Default::default();
    image.set_key_position(0, [0; 2]);
    assert_eq!(image.key_position, [i32::MIN + 16, i32::MAX]);
    image.set_size([33, -3]);
    assert_eq!(image.key_position, [17, -11]);
    assert!(!image.updates.needs_rebuild());
    image.set_size([16, 17]);
    image.set_key_position(0, [0; 2]);
    assert_eq!(image.key_position, [8, 18]);
    image.set_size([i32::MIN, i32::MAX]);
    assert_eq!(image.key_position, [i32::MAX - 15, i32::MAX - 8]);
}

#[test]
fn cimage_resolves_size_before_consuming_position_resource() {
    let mut image = fes::CImageState::default();
    image.apply(
        &fes::ObjectAttributes {
            position_resource: Some(b"crop:9".to_vec()),
            size_resource: Some(b"dimensions:7".to_vec()),
            ..Default::default()
        },
        0,
    );
    let mut requests = Vec::new();
    for _ in 0..2 {
        image
            .resolve_geometry(|reference| {
                requests.push((reference.name.clone(), reference.frame));
                Ok(katze_system2::hg3::StandardInfo {
                    size: [123, 45],
                    offset: [-3, 8],
                    canvas_size: [1000, 600],
                    base: [4, 5],
                    bit_depth: 32,
                    reserved: 0,
                })
            })
            .unwrap();
    }
    assert_eq!(
        requests,
        vec![
            (b"dimensions".to_vec(), 7),
            (b"crop".to_vec(), 9),
            (b"dimensions".to_vec(), 7)
        ]
    );
    assert_eq!(image.position, [-3.0, 8.0]);
    assert_eq!(image.size, [123, 45]);
    image.size_resource = None;
    image.position_resource = Some(fes::ImageReference::parse(b"missing"));
    assert!(
        image
            .resolve_geometry(|_| Err(katze_system2::Error::new(0, "resource unavailable")))
            .is_err()
    );
    assert_eq!(image.position_resource, None);
    assert_eq!(image.position, [-3.0, 8.0]);
}

#[test]
fn geometry_resources_use_frame_number_prefixes_instead_of_expressions() {
    for (input, name, frame) in [
        (b"asset".as_slice(), b"asset".as_slice(), 0),
        (b"asset:7", b"asset", 7),
        (b"asset:0x10tail", b"asset", 16),
        (b"asset:-0X2rest", b"asset", -2),
        (b"asset:+12+5", b"asset", 12),
        (b"asset:$4", b"asset", 0),
        (b"asset: 7", b"asset", 0),
        (b"asset:", b"asset", 0),
        (b"asset:4294967295", b"asset", -1),
        (b"asset\x82\xa0:3", b"asset\x82\xa0", 3),
    ] {
        let reference = fes::ImageReference::parse(input);
        assert_eq!(reference.name, name);
        assert_eq!(reference.frame, frame);
    }
}

#[test]
fn initialization_column_offsets_and_numeric_diagnostics_have_distinct_contracts() {
    let source = b"#OBJECT ID.100 DISP\nitem 99 1\n#START\nexit 0\n";
    let mut vm = fes::Interpreter::new(fes::Program::from_text(source).unwrap()).unwrap();
    let Step::Event(fes::Event::InitializeObject(initialization)) =
        vm.run(&mut State::new(8), 20).unwrap()
    else {
        panic!("expected display initialization");
    };
    assert_eq!(initialization.attributes.display, Some(1));
    assert!(initialization.attributes.frames.iter().all(Option::is_none));
    for source in [
        b"#OBJECT pos\nitem 1,2\n#START\nexit 0\n".as_slice(),
        b"#OBJECT DISP\nitem 50%\n#START\nexit 0\n",
    ] {
        let mut vm = fes::Interpreter::new(fes::Program::from_text(source).unwrap()).unwrap();
        assert!(vm.run(&mut State::new(8), 20).is_err());
    }
}

#[test]
fn subcommand_mapping_consumes_inputs_in_order_and_selects_defaults_by_index() {
    let source =
        b"#SUBCOMMAND\noneshot motion target p[11] p[22] : 9 [1] cxr [0] cxa\n#START\nexit 0\n";
    let mut vm = fes::Interpreter::new(fes::Program::from_text(source).unwrap()).unwrap();
    let mut state = State::new(8);
    let table = vm.subcommands(&state);
    let binding = &table.bindings[b"motion".as_slice()];
    let context = fes::SubcommandContext {
        color: 0x7b345678,
        ..Default::default()
    };
    for (input, expected) in [
        ("101 202", vec![9.0, 101.0, 52.0, 202.0, 123.0]),
        ("101", vec![9.0, 101.0, 52.0, 11.0, 123.0]),
        ("", vec![9.0, 22.0, 52.0, 11.0, 123.0]),
    ] {
        let arguments = vm.parameters(input.as_bytes(), &state).unwrap();
        assert_eq!(
            vm.subcommand_arguments(binding, &arguments, &context, &mut state)
                .unwrap(),
            expected
        );
    }
    let mut invalid = binding.clone();
    invalid.arguments = vec![fes::MappedArgument::Parameter(-1)];
    assert!(
        vm.subcommand_arguments(&invalid, &[], &context, &mut state)
            .is_err()
    );
}

#[test]
fn subcommand_conversion_preserves_fractional_context_and_float_operation_order() {
    let source = b"#SUBCOMMAND\noneshot motion target bx SY RX rz ca CX : [0] [1] [2] [3] [4] [5]\n#START\nexit 0\n";
    let mut vm = fes::Interpreter::new(fes::Program::from_text(source).unwrap()).unwrap();
    let mut state = State::new(8);
    state.shared[4] = 5;
    state.current = 77;
    let table = vm.subcommands(&state);
    let context = fes::SubcommandContext {
        base: [10.75, 20.25, -30.5],
        rotation: [0.25, -0.5, 1.5],
        scale: [1.25, 0.75, 2.5],
        color: 0x7b345678,
        ..Default::default()
    };
    let arguments = vm.parameters(b"@ 50% 50% 90 50% $4+=3", &state).unwrap();
    let values = vm
        .subcommand_arguments(
            &table.bindings[b"motion".as_slice()],
            &arguments,
            &context,
            &mut state,
        )
        .unwrap();
    let expected = [-0.75f32, 0.5, 0.0021816615, 0.07079637, -61.5, 0.0];
    assert_eq!(
        values.iter().map(|f| f.to_bits()).collect::<Vec<_>>(),
        expected.map(f32::to_bits)
    );
    assert_eq!(state.shared[4], 5);
    assert_eq!(state.current, 77);
}

#[test]
fn subcommand_current_overrides_event_bank_and_restores_state_after_errors() {
    let source = b"#START\nwait 0\n#button.PUSH_L\nwait 0\n";
    let mut vm = fes::Interpreter::new(fes::Program::from_text(source).unwrap()).unwrap();
    let mut state = State::new(8);
    state.current = 77;
    assert!(matches!(
        vm.run(&mut state, 20).unwrap(),
        Step::Event(fes::Event::Wait(0))
    ));
    assert!(vm.dispatch_event("button", 3, "PUSH_L", None).unwrap());
    vm.named.entry("value".into()).or_default().insert(0, 19);
    state.named.entry("value".into()).or_default().insert(0, 90);
    let binding = fes::Subcommand {
        mode: fes::SubcommandMode::Oneshot,
        name: b"test".to_vec(),
        target: b"target".to_vec(),
        parameters: vec![fes::SubcommandParameter {
            descriptor: "PX",
            default: b"@+value".to_vec(),
        }],
        arguments: vec![fes::MappedArgument::Parameter(0)],
    };
    let context = fes::SubcommandContext {
        position: [12.75, 0.0, 0.0],
        ..Default::default()
    };
    assert_eq!(
        vm.subcommand_arguments(&binding, &[], &context, &mut state)
            .unwrap(),
        [31.0]
    );
    assert!(
        vm.subcommand_arguments(&binding, &[b"@!".to_vec()], &context, &mut state)
            .is_err()
    );
    assert_eq!(state.current, 77);
    assert_eq!(state.named["value"][&0], 90);
    assert_eq!(vm.named["value"][&0], 19);
    assert_eq!(vm.event_context().unwrap().bank, 3);
}

#[test]
fn button_columns_keep_image_mask_input_type_and_enable_updates_distinct() {
    let mut button = fes::ButtonState::default();
    let attributes = fes::ObjectAttributes {
        size: Some([3, 4]),
        color: Some(123),
        enabled: Some(21),
        display: Some(1),
        mask: Some(9),
        object_type: Some(2),
        button_type: Some(4),
        ..Default::default()
    };
    assert!(
        button
            .apply(&attributes, 0, |name| Ok(name.to_vec()))
            .unwrap()
    );
    assert_eq!(button.image.base.size, [3, 4]);
    assert_eq!(button.image.base.color, -1);
    assert_eq!(
        (
            button.image.base.mask,
            button.image.base.button_type,
            button.mask_type
        ),
        (9, 4, 2)
    );
    assert_eq!(button.enabled, 21);
    assert!(button.image.base.updates.needs_rebuild());
    button.image.base.updates = Default::default();
    assert!(
        !button
            .apply(&attributes, 0, |name| Ok(name.to_vec()))
            .unwrap()
    );
    assert!(!button.image.base.updates.needs_rebuild());
    assert!(!button.image.base.updates.is_dirty());
    assert!(
        !button
            .apply(
                &fes::ObjectAttributes {
                    enabled: Some(0),
                    ..Default::default()
                },
                0,
                |name| Ok(name.to_vec())
            )
            .unwrap()
    );
    assert!(button.image.base.updates.is_dirty());
    assert_eq!(button.enabled, 0);
}
