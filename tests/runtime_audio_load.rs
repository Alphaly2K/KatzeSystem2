use katze_system2::{
    Error, Result,
    audio::{SoundFactory, SoundGroup, SoundGroups},
    expression::State,
    kcs::{Program, ResourceServices},
    memory::SharedMemory,
    resource::ResourceMode,
    runtime::{KcsFactory, ProgramLoader, RootObject, Runtime, kind},
    task::{Command, CommandBuffers},
};

const NAME: &[u8] = b"tone_%u[G8].ogg\0";

fn instruction(code: &mut Vec<u8>, opcode: u8, words: &[u32]) {
    code.push(opcode);
    for word in words {
        code.push(3);
        code.extend(word.to_le_bytes());
    }
}

struct Source {
    calls: Vec<(u32, Vec<u32>)>,
}

impl ProgramLoader for Source {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        let mut code = vec![];
        instruction(&mut code, 0x25, &[0xdeadbeef]);
        for (i, (index, args)) in self.calls.iter().enumerate() {
            for arg in args {
                instruction(&mut code, 0x25, &[*arg]);
            }
            instruction(&mut code, 0x2b, &[*index, args.len() as u32 * 4]);
            instruction(&mut code, 0x26, &[256 + i as u32 * 4]);
        }
        instruction(&mut code, 0x26, &[0]);
        instruction(&mut code, 0x3b, &[]);
        let mut data = vec![0; 512];
        data[8..12].copy_from_slice(&17u32.to_le_bytes());
        data[64..64 + NAME.len()].copy_from_slice(NAME);
        Ok(Some(Program::from_code(&code, data, 128)?))
    }
}

struct Resources;
impl ResourceServices for Resources {
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        Ok([category, b"/", name].concat())
    }
    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        assert_eq!(mode, ResourceMode::LooseFirst);
        Ok((name == b"sound/tone_17.ogg")
            .then(|| include_bytes!("fixtures/audio/mono-44100.ogg").to_vec()))
    }
}

fn setup(calls: Vec<(u32, Vec<u32>)>) -> Runtime<SoundFactory<KcsFactory<Source>>> {
    let mut factory = KcsFactory::new(Source { calls });
    let mut external = vec![0; 256];
    external[64..64 + NAME.len()].copy_from_slice(NAME);
    factory.external = SharedMemory::new(external).unwrap();
    let mut rt = Runtime::new(
        0,
        kind::ROOT,
        Box::new(RootObject::new(u32::MAX, 1)),
        State::new(0),
        SoundFactory {
            enabled: true,
            fallback: factory,
        },
    );
    rt.host_mut().sound_groups = Some(SoundGroups {
        entries: [(65536, 7, [4, 4]), (65537, 8, [10, 10]), (2, 0, [9, 9])]
            .into_iter()
            .map(|(key, id, range)| SoundGroup {
                key,
                id,
                range,
                label: vec![],
            })
            .collect(),
    });
    rt.set_resources(Resources);
    rt.create(0, kind::SOUND, b"sound", b"").unwrap();
    rt.create(0, kind::ORDINARY, b"caller", b"").unwrap();
    rt
}

#[test]
fn native_loads_preserve_unsigned_options_and_reach_all_four_group_variants() {
    for budget in [1, 4096] {
        for address in [64, 0x80000040] {
            let mut rt = setup(vec![
                (935, vec![5, u32::MAX, address, 1, 61]),
                (935, vec![6, 0, address, 2, u32::MAX]),
                (936, vec![65536, 0, address, 1, 1, 0x80000000]),
                (936, vec![65537, 0, address, u32::MAX, 2, 60]),
                (936, vec![7, u32::MAX, address, 0, 1, 1]),
                (936, vec![7, 0, address, 0, 2, 59]),
                (935, vec![7, 0, 63, 1, 1]),
            ]);
            rt.set_budget(budget, 1 << 20);
            let mut point = [0; 48];
            let name = b"sound/tone_17.ogg";
            point[..name.len()].copy_from_slice(name);
            point[32..40].copy_from_slice(&1i64.to_le_bytes());
            point[40..48].copy_from_slice(&20i64.to_le_bytes());
            rt.dispatch_command_with_buffers(
                0,
                Command {
                    target: 1,
                    sender: 0,
                    code: 199,
                    data: [0; 3],
                },
                &mut CommandBuffers {
                    data1: Some(&mut point),
                    ..Default::default()
                },
            )
            .unwrap();
            rt.update(0, 0).unwrap();
            let vm = rt.object(2).unwrap().interpreter().unwrap();
            assert!(vm.stack_bytes().is_empty());
            assert_eq!(vm.read(0, 4).unwrap(), 0xdeadbeefu32.to_le_bytes());
            let returned: Vec<_> = vm
                .read(256, 28)
                .unwrap()
                .chunks_exact(4)
                .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                .collect();
            assert_eq!(returned, [1, 1, 4, 10, 9, 9, 0]);
            let sound = rt.object(1).unwrap().audio_state().unwrap();
            for (channel, options, loops) in [
                (5, 1016, [1, 20]),
                (6, (u64::from(u32::MAX) * 1000 / 60) as u32, [0, -1]),
                (4, (0x80000000u64 * 1000 / 60) as u32, [0, -1]),
                (10, 1000, [0, -1]),
                (9, 983, [1, 20]),
            ] {
                let channel = sound.channel(channel).unwrap();
                assert_eq!(channel.file(), name);
                assert_eq!(channel.load_parameters(), [0, 0]);
                assert_eq!(channel.load_options(), options);
                assert_eq!(channel.loop_range(), loops);
            }
            let empty = sound.channel(7).unwrap();
            assert!(empty.pcm().is_none());
            assert_eq!(empty.load_options(), 16);
        }
    }
}

#[test]
fn native_loads_diagnose_uninitialized_results_and_short_arguments() {
    for (index, args, message) in [
        (935, vec![5, 0, 64, 0, 1], "选择器"),
        (936, vec![7, 0, 64, 1, 3, 1], "选择器"),
        (935, vec![5, 0, 64, 1], "范围"),
    ] {
        let mut rt = setup(vec![(index, args)]);
        let Error {
            message: actual, ..
        } = rt.update(0, 0).unwrap_err();
        assert!(actual.contains(message), "{actual}");
        assert!(
            rt.object(1)
                .unwrap()
                .audio_state()
                .unwrap()
                .channel(5)
                .unwrap()
                .pcm()
                .is_none()
        );
    }
}
