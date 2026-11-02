use katze_system2::{
    Result,
    expression::State,
    kcs::{CoreHost, Host, Interpreter, NativeReturn, Program},
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
    settings::SystemSettings,
};

fn settings() -> SystemSettings {
    SystemSettings {
        sound: [0x11; 0x15fc],
        message: [0x22; 0x48],
        effects: [0x33; 0x20],
        window: [0x44; 0x0c],
    }
}

fn arguments(values: &[u32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

#[test]
fn settings_reads_keep_zero_addresses_overlap_order_and_partial_failures() {
    let mut numbers = State::new(0);
    let mut host = CoreHost::new(&mut numbers);
    let mut vm = Interpreter::new(
        Program::from_code(&[], vec![0xaa; 6000], 0).unwrap(),
        vec![0xbb; 16],
    )
    .unwrap();
    let nulls = arguments(&[u32::MAX; 4]);
    assert_eq!(
        host.call(359, &nulls, &mut vm).unwrap(),
        NativeReturn::Continue
    );
    assert!(host.call(359, &arguments(&[0; 4]), &mut vm).is_err());
    host.system_settings = Some(settings());
    host.call(359, &arguments(&[0, 8, 16, 24]), &mut vm)
        .unwrap();
    let mut expected = vec![0x11; 0x15fc];
    expected[8..80].fill(0x22);
    expected[16..48].fill(0x33);
    expected[24..36].fill(0x44);
    assert_eq!(vm.read(0, expected.len()).unwrap(), expected);
    assert_eq!(
        vm.read(0x15fc, 6000 - 0x15fc).unwrap(),
        vec![0xaa; 6000 - 0x15fc]
    );
    assert_eq!(vm.read(0x80000000, 16).unwrap(), [0xbb; 16]);

    vm.write(0, &[0x55; 0x15fc]).unwrap();
    assert!(
        host.call(360, &arguments(&[0, 0x80000008, u32::MAX]), &mut vm)
            .is_err()
    );
    let state = host.system_settings.as_ref().unwrap();
    assert_eq!(state.sound, [0x55; 0x15fc]);
    assert_eq!(state.message, [0x22; 0x48]);
    assert_eq!(state.window, [0x44; 0x0c]);
    let snapshot = state.clone();
    assert!(host.call(360, &arguments(&[0; 4]), &mut vm).is_err());
    assert_eq!(host.system_settings.as_ref().unwrap(), &snapshot);
    assert!(
        host.call(
            359,
            &arguments(&[u32::MAX, u32::MAX, u32::MAX, 5990]),
            &mut vm
        )
        .is_err()
    );
}

struct Source;
impl ProgramLoader for Source {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        fn ins(code: &mut Vec<u8>, op: u8, words: &[u32]) {
            code.push(op);
            for word in words {
                code.push(3);
                code.extend(word.to_le_bytes());
            }
        }
        let mut code = vec![];
        ins(&mut code, 0x25, &[0xdeadbeef]);
        for value in [0, 5800, 5900] {
            ins(&mut code, 0x25, &[value]);
        }
        ins(&mut code, 0x2b, &[360, 12]);
        for value in [0, 0x80000000, 0x80000080, 0x800000c0] {
            ins(&mut code, 0x25, &[value]);
        }
        ins(&mut code, 0x2b, &[359, 16]);
        ins(&mut code, 0x26, &[6300]);
        ins(&mut code, 0x3b, &[]);
        let mut data = vec![0; 6400];
        data[..0x15fc].fill(0x31);
        data[5800..5800 + 0x48].fill(0x52);
        data[5900..5900 + 0x20].fill(0x73);
        Ok(Some(Program::from_code(&code, data, 128)?))
    }
}

#[test]
fn ordinary_settings_writeback_and_external_reads_share_runtime_state() {
    for budget in [1, 4096] {
        let mut factory = KcsFactory::new(Source);
        factory.external_bytes = 512;
        let mut rt = Runtime::new(
            0,
            kind::BASE,
            Box::new(BaseObject::new()),
            State::new(0),
            factory,
        );
        rt.set_budget(budget, 1 << 20);
        rt.host_mut().system_settings = Some(settings());
        let task = rt.create(0, kind::ORDINARY, b"settings", b"").unwrap();
        rt.update(0, 0).unwrap();
        let state = rt.host().system_settings.as_ref().unwrap();
        assert_eq!(state.sound, [0x31; 0x15fc]);
        assert_eq!(state.message, [0x52; 0x48]);
        assert_eq!(state.effects, [0x73; 0x20]);
        assert_eq!(state.window, [0x44; 0x0c]);
        let vm = rt.object(task).unwrap().interpreter().unwrap();
        assert!(vm.stack_bytes().is_empty());
        assert_eq!(vm.read(6300, 4).unwrap(), 0xdeadbeefu32.to_le_bytes());
        let mut expected = [0; 512];
        expected[..0x48].fill(0x52);
        expected[0x80..0xa0].fill(0x73);
        expected[0xc0..0xcc].fill(0x44);
        assert_eq!(vm.read(0x80000000, 512).unwrap(), expected);
    }
}
