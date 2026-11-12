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
            if matches!(index, 160 | 162 | 165 | 349 | 903 | 935 | 936) {
                instruction(&mut code, 0x26, &[256 + i as u32 * 4]);
            }
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
        Ok(match name {
            b"sound/tone_17.ogg" => Some(include_bytes!("fixtures/audio/mono-44100.ogg").to_vec()),
            b"tag/tone_17" => {
                let mut row = vec![0; 24];
                row[..11].copy_from_slice(b"tone_17.ogg");
                row[20..].copy_from_slice(&37u32.to_le_bytes());
                Some(row)
            }
            _ => None,
        })
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
            let source = sound.channel(5).unwrap().pcm().unwrap().samples()[..3].to_vec();
            let play = Command {
                target: 1,
                sender: 0,
                code: 183,
                data: [5, 1, 0],
            };
            rt.dispatch_command(0, play).unwrap();
            let output = rt
                .object_mut(1)
                .unwrap()
                .audio_state_mut()
                .unwrap()
                .read_channel(5, 44_808)
                .unwrap();
            assert!(output.samples()[..44_805].iter().all(|sample| *sample == 0));
            assert_eq!(&output.samples()[44_805..], source);
            rt.dispatch_command(0, Command { code: 185, ..play })
                .unwrap();
            rt.dispatch_command(0, play).unwrap();
            let output = rt
                .object_mut(1)
                .unwrap()
                .audio_state_mut()
                .unwrap()
                .read_channel(5, 3)
                .unwrap();
            assert_eq!(output.samples(), source);
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

#[test]
fn native_controls_reach_real_playback_and_keep_void_results_off_the_stack() {
    for budget in [1, 4096] {
        let mut rt = setup(vec![
            (935, vec![5, 0, 64, 1, 0]),
            (161, vec![5, (-69i32) as u32]),
            (164, vec![5, 234]),
            (162, vec![5]),
            (165, vec![5]),
            (157, vec![5, 1]),
            (160, vec![5]),
            (158, vec![5]),
            (160, vec![5]),
            (157, vec![5, 0]),
            (160, vec![5]),
            (162, vec![5]),
            (159, vec![5]),
            (160, vec![5]),
            (156, vec![5]),
            (160, vec![5]),
            (162, vec![5]),
            (165, vec![5]),
        ]);
        rt.set_budget(budget, 4096);
        rt.update(0, 0).unwrap();
        let vm = rt.object(2).unwrap().interpreter().unwrap();
        assert!(vm.stack_bytes().is_empty());
        assert_eq!(vm.read(0, 4).unwrap(), 0xdeadbeefu32.to_le_bytes());
        for (slot, expected) in [
            (0, 1),
            (3, (-69i32) as u32),
            (4, 100),
            (6, 1),
            (8, 0),
            (10, 1),
            (11, (-69i32) as u32),
            (13, 0),
            (15, 0),
            (16, 0),
            (17, 0),
        ] {
            assert_eq!(vm.read(256 + slot * 4, 4).unwrap(), expected.to_le_bytes());
        }
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

#[test]
fn native_spatial_and_ramp_controls_preserve_float_bits_and_reach_real_channels() {
    for budget in [1, 4096] {
        let mut rt = setup(vec![
            (935, vec![5, 0, 64, 1, 0]),
            (167, vec![5, 1.0f32.to_bits(), 0x7fa12345]),
            (168, vec![5, (-1.0f32).to_bits(), 0x7fa45678, 0x80000000]),
            (165, vec![5]),
            (163, vec![5, 20, 5]),
            (166, vec![5, 100, 5]),
            (347, vec![5, 0x80000000, 0x7fa12345, 0x7f800000]),
            (348, vec![5, 0xffa23456]),
            (349, vec![5]),
        ]);
        rt.set_budget(budget, 4096);
        rt.update(0, 0).unwrap();
        let sound = rt
            .object(1)
            .unwrap()
            .audio_state()
            .unwrap()
            .channel(5)
            .unwrap();
        assert_eq!(
            sound.distance_range().unwrap().map(f32::to_bits),
            [100.0f32.to_bits(), 0x7fa12345]
        );
        assert_eq!(
            sound.position().unwrap().map(f32::to_bits),
            [(-1.0f32).to_bits(), 0x7fa45678, 0x80000000]
        );
        assert_eq!(sound.pan(), Some(-100));
        assert_eq!(
            sound.velocity().unwrap().map(f32::to_bits),
            [0x80000000, 0x7fa12345, 0x7f800000]
        );
        assert_eq!(sound.doppler_factor().unwrap().to_bits(), 0xffa23456);
        let vm = rt.object(2).unwrap().interpreter().unwrap();
        assert!(vm.stack_bytes().is_empty());
        assert_eq!(vm.read(0, 4).unwrap(), 0xdeadbeefu32.to_le_bytes());
        assert_eq!(vm.read(268, 4).unwrap(), (-100i32).to_le_bytes());
        assert_eq!(vm.read(288, 4).unwrap(), 1u32.to_le_bytes());
        rt.update(1, 1).unwrap();
        let sound = rt
            .object(1)
            .unwrap()
            .audio_state()
            .unwrap()
            .channel(5)
            .unwrap();
        assert_eq!((sound.volume(), sound.pan()), (Some(80), Some(-60)));
    }
}

#[test]
fn formatted_tag_queries_use_both_vm_regions_and_keep_tags_after_unload() {
    for budget in [1, 4096] {
        let mut runtime = setup(vec![
            (935, vec![5, u32::MAX, 64, 1, 0]),
            (903, vec![5, 64]),
            (903, vec![5, 0x80000040]),
            (903, vec![u32::MAX, 64]),
            (156, vec![5]),
            (903, vec![5, 64]),
            (935, vec![5, u32::MAX, 63, 1, 0]),
            (903, vec![5, 64]),
        ]);
        runtime.set_budget(budget, 1 << 20);
        runtime.update(0, 1).unwrap();
        let vm = runtime.object(2).unwrap().interpreter().unwrap();
        for index in [1, 2, 3, 5] {
            assert_eq!(
                vm.read(256 + index * 4, 4).unwrap().as_ref(),
                37u32.to_le_bytes()
            );
        }
        assert_eq!(
            vm.read(256 + 7 * 4, 4).unwrap().as_ref(),
            0u32.to_le_bytes()
        );
        assert_eq!(vm.read(0, 4).unwrap().as_ref(), 0xdeadbeefu32.to_le_bytes());
    }
}
