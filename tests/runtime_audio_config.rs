use katze_system2::{
    Error, Result,
    audio::{SoundConfig, SoundPart},
    expression::State,
    kcs::{OrdinaryTask, Program},
    runtime::{ObjectFactory, OrdinaryObject, Runtime, TaskObject, kind},
};

struct Reject;
impl ObjectFactory for Reject {
    fn create(&mut self, _: u32, _: u32, _: &[u8]) -> Result<Box<dyn TaskObject>> {
        Err(Error::new(0, "unexpected object creation"))
    }
}

fn ins(code: &mut Vec<u8>, opcode: u8, values: &[u32]) {
    code.push(opcode);
    for value in values {
        code.push(3);
        code.extend(value.to_le_bytes());
    }
}

#[test]
fn native_configuration_queries_keep_three_lookup_modes_and_stack_results_across_budgets() {
    let config = SoundConfig {
        master_volume: Some(47),
        master_play_enabled: Some(-2),
        master_system_voice: Some(-3),
        master_mute: -1,
        parts: vec![
            SoundPart {
                name: b"SELECTOR".to_vec(),
                id: 11,
                same_id: 2,
                master_volume: 65,
                volume: 37,
                play_enabled: -3,
                system_voice: 2,
                voice_volume: 89,
                skip_volume: -7,
                mute: -1,
                children: vec![SoundPart {
                    type_name: b"SELECTOR".to_vec(),
                    master_volume: 73,
                    volume: 41,
                    play_enabled: 4,
                    system_voice: -5,
                    voice_volume: 91,
                    skip_volume: 11,
                    mute: 2,
                    ..Default::default()
                }],
                ..Default::default()
            },
            SoundPart {
                file_prefix: b"SELECTOR".to_vec(),
                id: -7,
                same_id: i32::MIN,
                master_volume: 59,
                volume: 43,
                play_enabled: 6,
                system_voice: 7,
                voice_volume: 93,
                skip_volume: 13,
                mute: 0,
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let expected: [i32; 36] = [
        47,
        -2,
        -3,
        0,
        43,
        6,
        7,
        59,
        93,
        13,
        0,
        41,
        4,
        -5,
        73,
        91,
        11,
        2,
        37,
        -3,
        2,
        65,
        89,
        -7,
        0,
        11,
        -1,
        0,
        -1,
        100,
        0,
        47,
        i32::MIN,
        -7,
        0,
        -1,
    ];
    for budget in [1, 4096] {
        for address in [64, 0x8000_0040] {
            let mut code = Vec::new();
            ins(&mut code, 0x25, &[0x1357_9bdf]);
            let mut queries: Vec<_> = (939..=964)
                .map(|index| (index, if index < 943 { u32::MAX } else { address }))
                .collect();
            let missing = (address & 0x8000_0000) | 80;
            queries.extend([948, 951, 955, 956, 963, 964].map(|index| (index, missing)));
            queries.extend([
                (1000, address),
                (1001, address),
                (1000, missing),
                (1001, missing),
            ]);
            for (i, (index, address)) in queries.into_iter().enumerate() {
                ins(&mut code, 0x25, &[address]);
                ins(&mut code, 0x25, &[u32::MAX]);
                ins(&mut code, 0x2b, &[index, 8]);
                ins(&mut code, 0x26, &[128 + i as u32 * 4]);
            }
            ins(&mut code, 0x3b, &[]);
            let mut data = vec![0; 288];
            data[16..25].copy_from_slice(b"selector\0");
            data[64..72].copy_from_slice(b"%s[G16]\0");
            data[80..88].copy_from_slice(b"missing\0");
            let task =
                OrdinaryTask::new(Program::from_code(&code, data.clone(), 64).unwrap(), data)
                    .unwrap();
            let mut rt = Runtime::new(
                0,
                kind::ORDINARY,
                Box::new(OrdinaryObject::new(task)),
                State::new(0),
                Reject,
            );
            rt.host_mut().sound_config = Some(config.clone());
            rt.host_mut().sound_settings_dirty = 0x1234;
            rt.host_mut().sound_config_enabled = u32::from(address >> 31 == 0);
            rt.set_budget(budget, 4096);
            let mut done = false;
            for _ in 0..256 {
                if rt.update(100, 1).unwrap() {
                    done = true;
                    break;
                }
            }
            assert!(done);
            let vm = rt.object(0).unwrap().interpreter().unwrap();
            let actual: Vec<_> = vm
                .read(128, expected.len() * 4)
                .unwrap()
                .chunks_exact(4)
                .map(|b| i32::from_le_bytes(b.try_into().unwrap()))
                .collect();
            assert_eq!(actual, expected);
            assert_eq!(vm.stack_bytes(), 0x1357_9bdfu32.to_le_bytes());
            assert_eq!(vm.read(address, 8).unwrap(), b"%s[G16]\0");
            assert_eq!(rt.host().sound_settings_dirty, 0x1234);
        }
    }
}
