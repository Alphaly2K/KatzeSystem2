use katze_system2::{
    Error, Result,
    audio::{SoundFactory, SoundGroup, SoundGroups},
    expression::State,
    kcs::ResourceServices,
    resource::ResourceMode,
    runtime::{BaseObject, ObjectFactory, Runtime, TaskObject, kind},
    task::{Command, CommandBuffers},
};
use std::{cell::RefCell, rc::Rc};

const MONO: &[u8] = include_bytes!("fixtures/audio/mono-44100.ogg");
const STEREO: &[u8] = include_bytes!("fixtures/audio/stereo-48000.ogg");
const LONG_MONO: &[u8] = include_bytes!("fixtures/audio/mono-8000-long.ogg");

#[derive(Debug, PartialEq)]
enum Event {
    Resolve(Vec<u8>, Vec<u8>),
    Read(Vec<u8>),
}

#[derive(Default)]
struct Source {
    events: Vec<Event>,
    fail: bool,
    metadata: bool,
}

struct Resources(Rc<RefCell<Source>>);

impl ResourceServices for Resources {
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        self.0
            .borrow_mut()
            .events
            .push(Event::Resolve(category.to_vec(), name.to_vec()));
        Ok([category, b"/", name, b".", category].concat())
    }
    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        assert_eq!(mode, ResourceMode::LooseFirst);
        let mut source = self.0.borrow_mut();
        source.events.push(Event::Read(name.to_vec()));
        if source.fail {
            source.fail = false;
            return Err(Error::new(0, "合成资源读取失败"));
        }
        Ok(match name {
            b"audio/tone.ogg" => Some(MONO.to_vec()),
            b"audio/stereo.ogg" => Some(STEREO.to_vec()),
            b"audio/long.ogg" => Some(LONG_MONO.to_vec()),
            b"audio/corrupt.ogg" => Some(vec![1, 2, 3]),
            b"tag/tone.tag" if source.metadata => Some(vec![1]),
            _ => None,
        })
    }
}

struct Reject;
impl ObjectFactory for Reject {
    fn create(&mut self, _: u32, _: u32, _: &[u8]) -> Result<Box<dyn TaskObject>> {
        Err(Error::new(0, "合成工厂不创建此类型"))
    }
}

fn setup() -> (Runtime<SoundFactory<Reject>>, Rc<RefCell<Source>>) {
    setup_with_groups(None)
}

fn setup_with_groups(
    groups: Option<SoundGroups>,
) -> (Runtime<SoundFactory<Reject>>, Rc<RefCell<Source>>) {
    let source = Rc::new(RefCell::new(Source::default()));
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        SoundFactory {
            enabled: true,
            fallback: Reject,
        },
    );
    runtime.set_resources(Resources(source.clone()));
    runtime.host_mut().sound_groups = groups;
    assert_eq!(runtime.create(0, kind::SOUND, b"sound", b"").unwrap(), 1);
    (runtime, source)
}

fn command(code: u32, channel: u32) -> Command {
    Command {
        target: 1,
        sender: 0,
        code,
        data: [channel, 0, 0],
    }
}

fn record(channel: u32, mode: i32, duration: i32, name: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0; 0x120];
    bytes[..4].copy_from_slice(&channel.to_le_bytes());
    bytes[12..12 + name.len()].copy_from_slice(name);
    bytes[0x110..0x114].copy_from_slice(&1u32.to_le_bytes());
    bytes[0x118..0x11c].copy_from_slice(&mode.to_le_bytes());
    bytes[0x11c..0x120].copy_from_slice(&duration.to_le_bytes());
    bytes
}

fn load(runtime: &mut Runtime<SoundFactory<Reject>>, bytes: &mut [u8]) -> Result<u32> {
    runtime.dispatch_command_with_buffers(
        0,
        command(180, 0),
        &mut CommandBuffers {
            data1: Some(bytes),
            ..Default::default()
        },
    )
}

fn audio(runtime: &Runtime<SoundFactory<Reject>>) -> &katze_system2::audio::SoundState {
    runtime.object(1).unwrap().audio_state().unwrap()
}

fn read_channel(
    runtime: &mut Runtime<SoundFactory<Reject>>,
    frames: usize,
) -> katze_system2::audio::Pcm {
    runtime
        .object_mut(1)
        .unwrap()
        .audio_state_mut()
        .unwrap()
        .read_channel(5, frames)
        .unwrap()
}

fn ramp_status(runtime: &mut Runtime<SoundFactory<Reject>>, channel: u32) -> u32 {
    let mut bytes = [0xa5; 8];
    assert_eq!(
        runtime
            .dispatch_command_with_buffers(
                0,
                command(198, channel),
                &mut CommandBuffers {
                    data1: Some(&mut bytes),
                    ..Default::default()
                },
            )
            .unwrap(),
        0
    );
    assert_eq!(&bytes[4..], &[0xa5; 4]);
    u32::from_le_bytes(bytes[..4].try_into().unwrap())
}

#[test]
fn volume_and_pan_ramps_share_time_and_immediate_settings_cancel_their_own_ramp() {
    let (mut runtime, _) = setup();
    load(&mut runtime, &mut record(5, 0, 0, b"audio/tone.ogg")).unwrap();
    let set = |runtime: &mut Runtime<SoundFactory<Reject>>, code, value| {
        runtime
            .dispatch_command(
                0,
                Command {
                    data: [5, value, 0],
                    ..command(code, 5)
                },
            )
            .unwrap();
    };
    set(&mut runtime, 189, 40 | (4 << 16));
    set(&mut runtime, 192, 80 | (4 << 16));
    assert_eq!(ramp_status(&mut runtime, 5), 1);
    for (now, volume, pan) in [(1, 80, 20), (2, 60, 35), (4, 40, 57), (6, 40, 80)] {
        runtime.update(now, 1).unwrap();
        let sound = audio(&runtime).channel(5).unwrap();
        assert_eq!((sound.volume(), sound.pan()), (Some(volume), Some(pan)));
        assert_eq!(sound.requested_volume(), 0.0);
        assert_eq!(ramp_status(&mut runtime, 5), u32::from(now < 4));
    }
    set(&mut runtime, 189, 90 | (20 << 16));
    set(&mut runtime, 192, (-80i16 as u16 as u32) | (20 << 16));
    set(&mut runtime, 187, 17);
    assert_eq!(ramp_status(&mut runtime, 5), 0);
    runtime.host_mut().sound_stereo = Some(0);
    set(&mut runtime, 190, 100);
    runtime.update(100, 1).unwrap();
    runtime.host_mut().sound_stereo = Some(1);
    runtime.update(101, 1).unwrap();
    let sound = audio(&runtime).channel(5).unwrap();
    assert_eq!((sound.volume(), sound.pan()), (Some(17), Some(80)));
    assert_eq!(sound.requested_volume(), 17.0);

    set(&mut runtime, 189, 80 | (50 << 16));
    set(&mut runtime, 192, 50 << 16);
    set(&mut runtime, 184, 0);
    runtime.update(200, 1).unwrap();
    let sound = audio(&runtime).channel(5).unwrap();
    assert_eq!((sound.volume(), sound.pan()), (Some(17), Some(80)));

    set(&mut runtime, 187, (-17i32) as u32);
    set(&mut runtime, 189, 100 | (10 << 16));
    runtime.update(201, 1).unwrap();
    assert_eq!(
        audio(&runtime).channel(5).unwrap().volume(),
        Some(-477_218_560)
    );
    runtime.update(211, 1).unwrap();
    assert_eq!(audio(&runtime).channel(5).unwrap().volume(), Some(100));
    assert_eq!(ramp_status(&mut runtime, 5), 0);
    assert_eq!(ramp_status(&mut runtime, u32::MAX), 0);
    runtime
        .dispatch_command(
            0,
            Command {
                data: [31, 10 << 16, 0],
                ..command(189, 31)
            },
        )
        .unwrap();
    assert_eq!(ramp_status(&mut runtime, 31), 1);
}

#[test]
fn velocity_and_doppler_preserve_bits_ignore_stereo_and_follow_resource_lifetime() {
    let (mut rt, _) = setup();
    let set = |rt: &mut Runtime<SoundFactory<Reject>>, code, bits: &[u32]| {
        let mut bytes = bits
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>();
        rt.dispatch_command_with_buffers(
            0,
            command(code, 5),
            &mut CommandBuffers {
                data1: Some(&mut bytes),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            bytes,
            bits.iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>()
        );
    };
    set(&mut rt, 196, &[1, 2, 3]);
    set(&mut rt, 197, &[4]);
    assert_eq!(audio(&rt).channel(5).unwrap().velocity(), None);
    assert_eq!(audio(&rt).channel(5).unwrap().doppler_factor(), None);
    load(&mut rt, &mut record(5, 0, 17, b"audio/tone.ogg")).unwrap();
    assert_eq!(audio(&rt).channel(5).unwrap().velocity(), Some([0.0; 3]));
    assert_eq!(audio(&rt).channel(5).unwrap().doppler_factor(), Some(1.0));
    load(&mut rt, &mut record(5, 1, 17, b"audio/tone.ogg")).unwrap();
    rt.host_mut().sound_stereo = Some(0);
    set(&mut rt, 196, &[0x80000000, 0x7fa12345, 0xff800000]);
    set(&mut rt, 197, &[0xffa23456]);
    let sound = audio(&rt).channel(37).unwrap();
    assert_eq!(
        sound.velocity().unwrap().map(f32::to_bits),
        [0x80000000, 0x7fa12345, 0xff800000]
    );
    assert_eq!(sound.doppler_factor().unwrap().to_bits(), 0xffa23456);
    assert_eq!(sound.pan(), Some(0));
    assert_eq!(audio(&rt).channel(5).unwrap().velocity(), Some([0.0; 3]));
    load(&mut rt, &mut record(37, 0, 17, b"audio/tone.ogg")).unwrap();
    assert_eq!(audio(&rt).channel(37).unwrap().velocity(), Some([0.0; 3]));
    assert_eq!(audio(&rt).channel(37).unwrap().doppler_factor(), Some(1.0));
    rt.dispatch_command(0, command(182, 5)).unwrap();
    assert_eq!(audio(&rt).channel(37).unwrap().velocity(), None);
    assert_eq!(audio(&rt).channel(37).unwrap().doppler_factor(), None);
}

#[test]
fn load_variants_select_group_keys_or_categories_and_keep_distinct_loop_rules() {
    let groups = SoundGroups {
        entries: [(7, 0, [4, 5]), (2, 0, [6, 6]), (0, 9, [7, 7])]
            .into_iter()
            .map(|(key, id, range)| SoundGroup {
                key,
                id,
                label: vec![],
                range,
            })
            .collect(),
    };
    let (mut runtime, _) = setup_with_groups(Some(groups));
    let mut point = [0; 48];
    point[..14].copy_from_slice(b"audio/tone.ogg");
    point[32..40].copy_from_slice(&3i64.to_le_bytes());
    point[40..48].copy_from_slice(&99i64.to_le_bytes());
    runtime
        .dispatch_command_with_buffers(
            0,
            command(199, 0),
            &mut CommandBuffers {
                data1: Some(&mut point),
                ..Default::default()
            },
        )
        .unwrap();
    for (code, expected, range) in [
        (206, 4, [3, 99]),
        (207, 4, [0, -1]),
        (208, 6, [3, 99]),
        (209, 6, [3, 99]),
    ] {
        let mut bytes = record(55, 0, 17, b"audio/tone.ogg");
        bytes[4..8].copy_from_slice(&7u32.to_le_bytes());
        assert_eq!(
            runtime
                .dispatch_command_with_buffers(
                    0,
                    command(code, 0),
                    &mut CommandBuffers {
                        data1: Some(&mut bytes),
                        ..Default::default()
                    },
                )
                .unwrap(),
            expected
        );
        let sound = audio(&runtime).channel(expected).unwrap();
        assert!(sound.is_grouped());
        assert_eq!(sound.loop_range(), range);
    }
    for (code, length) in [(207, 8), (208, 12), (209, 12)] {
        let mut missing = vec![0xff; length];
        assert_eq!(
            runtime
                .dispatch_command_with_buffers(
                    0,
                    command(code, 0),
                    &mut CommandBuffers {
                        data1: Some(&mut missing),
                        ..Default::default()
                    },
                )
                .unwrap(),
            u32::MAX
        );
    }
}

#[test]
fn alternate_direct_load_forces_whole_playback_and_links_its_own_spare_channel() {
    let (mut runtime, _) = setup();
    let pcm = katze_system2::audio::decode_vorbis(LONG_MONO).unwrap();
    let mut bytes = record(5, 0, 0, b"audio/long.ogg");
    let invoke = |runtime: &mut Runtime<SoundFactory<Reject>>, bytes: &mut [u8]| {
        runtime.dispatch_command_with_buffers(
            0,
            command(181, 64),
            &mut CommandBuffers {
                data1: Some(bytes),
                ..Default::default()
            },
        )
    };
    assert_eq!(invoke(&mut runtime, &mut bytes).unwrap(), 1);
    let mut play = command(183, 5);
    play.data[1] = 2;
    runtime.dispatch_command(0, play).unwrap();
    let output = read_channel(&mut runtime, pcm.frames() + 10);
    assert_eq!(&output.samples()[..pcm.samples().len()], pcm.samples());
    assert_eq!(
        &output.samples()[pcm.samples().len()..],
        &pcm.samples()[..10]
    );
    assert_eq!(audio(&runtime).channel(5).unwrap().completed_passes(), 1);
    for (logical, expected) in [(5u32, 37), (u32::MAX, 31)] {
        let mut bytes = record(logical, 1, 17, b"audio/tone.ogg");
        assert_eq!(invoke(&mut runtime, &mut bytes).unwrap(), 1);
        let sound = audio(&runtime).channel(expected).unwrap();
        assert_eq!(sound.linked_channel(), Some(expected));
        assert_eq!(sound.load_parameters(), [1, 17]);
        assert!(!sound.is_grouped());
    }
    assert_eq!(audio(&runtime).channel(5).unwrap().linked_channel(), None);
}

#[test]
fn group_names_follow_live_configuration_while_channel_ranges_keep_the_creation_snapshot() {
    let groups = SoundGroups {
        entries: vec![
            SoundGroup {
                key: 7,
                id: 7,
                label: b"se".to_vec(),
                range: [4, 5],
            },
            SoundGroup {
                key: 7,
                id: 7,
                label: b"alias".to_vec(),
                range: [10, 11],
            },
            SoundGroup {
                key: 3,
                id: 3,
                label: b"SE".to_vec(),
                range: [12, 15],
            },
            SoundGroup {
                key: 19,
                id: 19,
                label: b"embedded\0suffix".to_vec(),
                range: [20, 20],
            },
            SoundGroup {
                key: 23,
                id: 23,
                label: vec![0x82, 0xa0],
                range: [21, 21],
            },
        ],
    };
    let (mut runtime, source) = setup_with_groups(Some(groups));
    assert_eq!(audio(&runtime).group_range(7), Some([4, 5]));
    let query = |runtime: &mut Runtime<SoundFactory<Reject>>, name: &[u8]| {
        let mut name = [name, b"\0"].concat();
        runtime.dispatch_command_with_buffers(
            0,
            command(210, 0),
            &mut CommandBuffers {
                data1: Some(&mut name),
                ..Default::default()
            },
        )
    };
    assert_eq!(query(&mut runtime, b"Se").unwrap(), 1);
    assert_eq!(query(&mut runtime, b"SYSVOICE").unwrap(), 3);
    assert_eq!(query(&mut runtime, b"missing").unwrap(), 5);
    runtime.host_mut().sound_config_enabled = 1;
    for (name, expected) in [
        (b"se".as_slice(), 7),
        (b"se\0unused", 7),
        (b"SE", 3),
        (b"Se", 0),
        (b"alias", 7),
        (b"embedded", 0),
        (b"\x82\xa0", 23),
    ] {
        assert_eq!(query(&mut runtime, name).unwrap(), expected);
    }
    let groups = runtime.host_mut().sound_groups.as_mut().unwrap();
    groups.entries[0].label = b"changed".to_vec();
    groups.entries[0].range = [8, 9];
    assert_eq!(query(&mut runtime, b"changed").unwrap(), 7);
    assert_eq!(audio(&runtime).group_range(7), Some([4, 5]));
    assert_eq!(runtime.create(0, kind::SOUND, b"later", b"").unwrap(), 2);
    assert_eq!(
        runtime
            .object(2)
            .unwrap()
            .audio_state()
            .unwrap()
            .group_range(7),
        Some([8, 9])
    );
    runtime.host_mut().sound_groups = None;
    assert!(query(&mut runtime, b"se").is_err());
    assert!(source.borrow().events.is_empty());
}

#[test]
fn grouped_loads_allocate_physical_channels_recycle_busy_sources_and_preserve_failed_load_state() {
    let groups = SoundGroups {
        entries: [
            (7, [4, 5]),
            (7, [10, 11]),
            (9, [6, 6]),
            (11, [7, 6]),
            (13, [64, 64]),
        ]
        .into_iter()
        .map(|(id, range)| SoundGroup {
            key: id,
            id,
            range,
            label: vec![],
        })
        .collect(),
    };
    let (mut runtime, source) = setup_with_groups(Some(groups));
    let load_group =
        |runtime: &mut Runtime<SoundFactory<Reject>>, group: u32, mode, name: &[u8]| {
            let mut bytes = record(55, mode, 17, name);
            bytes[4..8].copy_from_slice(&group.to_le_bytes());
            runtime.dispatch_command_with_buffers(
                0,
                command(206, 0),
                &mut CommandBuffers {
                    data1: Some(&mut bytes),
                    ..Default::default()
                },
            )
        };
    let play = |runtime: &mut Runtime<SoundFactory<Reject>>, channel| {
        let mut request = command(183, channel);
        request.data[1] = 1;
        runtime.dispatch_command(0, request).unwrap();
    };
    let mut point = [0; 48];
    point[..14].copy_from_slice(b"audio/tone.ogg");
    point[32..40].copy_from_slice(&3i64.to_le_bytes());
    point[40..].copy_from_slice(&99i64.to_le_bytes());
    runtime
        .dispatch_command_with_buffers(
            0,
            command(199, 0),
            &mut CommandBuffers {
                data1: Some(&mut point),
                ..Default::default()
            },
        )
        .unwrap();
    runtime.update(10, 1).unwrap();
    assert_eq!(
        load_group(&mut runtime, 7, 0, b"audio/tone.ogg").unwrap(),
        4
    );
    play(&mut runtime, 4);
    assert_eq!(
        load_group(&mut runtime, 7, 0, b"audio/stereo.ogg").unwrap(),
        5
    );
    play(&mut runtime, 5);
    assert_eq!(
        load_group(&mut runtime, 7, 0, b"audio/tone.ogg").unwrap(),
        4
    );
    assert!(!audio(&runtime).channel(4).unwrap().is_playing());
    assert!(audio(&runtime).channel(5).unwrap().is_playing());
    assert!(audio(&runtime).channel(4).unwrap().is_grouped());
    assert_eq!(audio(&runtime).channel(4).unwrap().loop_range(), [3, 99]);
    assert!(audio(&runtime).channel(10).unwrap().pcm().is_none());
    runtime.host_mut().system_time_ms = 321;
    runtime.dispatch_command(0, command(218, 4)).unwrap();
    runtime.update(20, 1).unwrap();
    source.borrow_mut().events.clear();
    assert_eq!(
        load_group(&mut runtime, 7, 0, b"audio/missing.ogg").unwrap(),
        u32::MAX
    );
    let channel = audio(&runtime).channel(4).unwrap();
    assert!(channel.pcm().is_none());
    assert!(channel.is_grouped());
    assert_eq!(channel.started_tick(), 321);
    assert_eq!(channel.load_parameters(), [0, 17]);
    assert_eq!(channel.loop_range(), [3, 99]);
    assert_eq!(audio(&runtime).channel_time(4).unwrap(), 10);
    assert_eq!(
        source.borrow().events,
        [Event::Read(b"audio/missing.ogg".to_vec())]
    );
    let mut unknown = [0; 8];
    unknown[4..].copy_from_slice(&999u32.to_le_bytes());
    assert_eq!(
        runtime
            .dispatch_command_with_buffers(
                0,
                command(206, 0),
                &mut CommandBuffers {
                    data1: Some(&mut unknown),
                    ..Default::default()
                }
            )
            .unwrap(),
        u32::MAX
    );
    assert_eq!(source.borrow().events.len(), 1);
    assert_eq!(
        load_group(&mut runtime, 9, -3, b"audio/tone.ogg").unwrap(),
        38
    );
    assert_eq!(
        audio(&runtime).channel(38).unwrap().linked_channel(),
        Some(38)
    );
    assert_eq!(audio(&runtime).channel(6).unwrap().linked_channel(), None);
    assert!(audio(&runtime).channel(38).unwrap().is_grouped());
    runtime.dispatch_command(0, command(182, 38)).unwrap();
    assert!(audio(&runtime).channel(38).unwrap().pcm().is_some());
    assert_eq!(
        audio(&runtime).channel(38).unwrap().started_tick(),
        u32::MAX
    );
    // 播放中的组通道置第一类分配标记即启动延迟释放，通道保持播放；
    // 已有标记时新请求不再修改。
    assert_eq!(
        load_group(&mut runtime, 7, 0, b"audio/tone.ogg").unwrap(),
        4
    );
    play(&mut runtime, 4);
    assert!(audio(&runtime).channel(4).unwrap().is_playing());
    runtime.dispatch_command(0, command(182, 5)).unwrap();
    assert!(audio(&runtime).channel(5).unwrap().is_playing());
    runtime.dispatch_command(0, command(182, 5)).unwrap();
    assert!(audio(&runtime).channel(5).unwrap().is_playing());
    // 全忙回收优先选择带释放标记的通道。
    assert_eq!(
        load_group(&mut runtime, 7, 0, b"audio/stereo.ogg").unwrap(),
        5
    );
    assert!(!audio(&runtime).channel(5).unwrap().is_playing());
    assert_eq!(
        load_group(&mut runtime, 11, 0, b"audio/tone.ogg").unwrap(),
        7
    );
    assert!(load_group(&mut runtime, 13, 0, b"audio/tone.ogg").is_err());
}

#[test]
fn streaming_task_count_changes_preserve_samples_prefetched_with_the_registered_loop() {
    let expected = katze_system2::audio::decode_vorbis(LONG_MONO).unwrap();
    assert_eq!(expected.frames(), 40_000);
    let (mut runtime, _) = setup();
    let mut point = [0; 48];
    point[..14].copy_from_slice(b"audio/long.ogg");
    point[32..40].copy_from_slice(&1000i64.to_le_bytes());
    point[40..].copy_from_slice(&35_000i64.to_le_bytes());
    runtime
        .dispatch_command_with_buffers(
            0,
            command(199, 0),
            &mut CommandBuffers {
                data1: Some(&mut point),
                ..Default::default()
            },
        )
        .unwrap();
    load(&mut runtime, &mut record(5, 0, 0, b"audio/long.ogg")).unwrap();
    let mut play = command(183, 5);
    play.data[1] = 2;
    runtime.dispatch_command(0, play).unwrap();
    assert_eq!(
        read_channel(&mut runtime, 4000).samples(),
        &expected.samples()[..4000]
    );
    assert_eq!(audio(&runtime).channel(5).unwrap().remaining_plays(), 1);
    let mut count = command(219, 5);
    count.data[1] = 1;
    runtime.dispatch_command(0, count).unwrap();
    assert_eq!(
        read_channel(&mut runtime, 28_000).samples(),
        &expected.samples()[4000..32_000]
    );
    let prefetched = read_channel(&mut runtime, 4000);
    assert_eq!(
        &prefetched.samples()[..3000],
        &expected.samples()[32_000..35_000]
    );
    assert_eq!(
        &prefetched.samples()[3000..],
        &expected.samples()[1000..2000]
    );
    assert_eq!(audio(&runtime).channel(5).unwrap().completed_passes(), 0);
    runtime.dispatch_command(0, command(185, 5)).unwrap();
    play.data[1] = 1;
    runtime.dispatch_command(0, play).unwrap();
    assert_eq!(
        read_channel(&mut runtime, 4000).samples(),
        &expected.samples()[..4000]
    );
}

#[test]
fn fes_play_starts_the_real_channel_and_reports_completion_after_sample_consumption() {
    use katze_system2::{
        audio::{SoundConfig, SoundPart},
        fes::Program,
        runtime::fes::{ObjectState, Scene},
    };
    for budget in [1, 4096] {
        let (mut runtime, _) = setup();
        runtime.numbers_mut().shared.resize(1, 0);
        runtime.set_budget(budget, 10_000);
        load(&mut runtime, &mut record(5, 0, 0, b"audio/tone.ogg")).unwrap();
        runtime.host_mut().sound_config_enabled = 1;
        runtime.host_mut().system_time_ms = 321;
        runtime.host_mut().sound_config = Some(SoundConfig {
            master_volume: Some(100),
            parts: vec![SoundPart {
                file_prefix: b"audio/".to_vec(),
                volume: 65,
                play_enabled: 1,
                mute: 0,
                ..Default::default()
            }],
            ..Default::default()
        });
        let mut scene = Scene::new(
            Program::from_text(b"#DEFINE\nSOUND music\n#OBJECT CHANNEL\nmusic 5\n#START\nmusic play 1\nwait 99\n#music.PLAY\n$0+=1\n").unwrap(),
            runtime.host().handle_allocator(),
        ).unwrap();
        assert!(scene.prepare(runtime.numbers_mut(), 4096).unwrap());
        let objects = scene.objects_mut().unwrap();
        objects.set_sound_task(1);
        let id = objects.lookup("music", 0).unwrap();
        let ObjectState::Sound(sound) = &mut objects.get_mut(id).unwrap().state else {
            panic!()
        };
        sound.file = b"audio/tone.ogg".to_vec();
        let mut done = false;
        for _ in 0..128 {
            if runtime.update_fes_scene(0, &mut scene, 10).unwrap() {
                done = true;
                break;
            }
        }
        assert!(done);
        assert_eq!(runtime.numbers().shared[0], 1);
        assert_eq!(runtime.dispatch_command(0, command(186, 5)).unwrap(), 1);
        assert_eq!(audio(&runtime).channel(5).unwrap().volume(), Some(65));
        assert_eq!(audio(&runtime).channel(5).unwrap().started_tick(), 321);
        let expected = katze_system2::audio::decode_vorbis(MONO).unwrap();
        let output = read_channel(&mut runtime, expected.frames() + 2);
        assert_eq!(&output.samples()[..expected.frames()], expected.samples());
        assert_eq!(&output.samples()[expected.frames()..], [0, 0]);
        assert_eq!(runtime.dispatch_command(0, command(186, 5)).unwrap(), 0);
        assert_eq!(runtime.numbers().shared[0], 1);
        let ObjectState::Sound(sound) = &scene.objects().get(id).unwrap().state else {
            panic!()
        };
        assert_eq!(sound.started_tick(), 321);
        assert_eq!(sound.action_request(), None);
    }
}

#[test]
fn buffered_playback_consumes_pcm_and_keeps_pause_stop_repeat_and_tick_semantics() {
    let (mut runtime, _) = setup();
    let expected = katze_system2::audio::decode_vorbis(MONO).unwrap();
    let set = |runtime: &mut Runtime<SoundFactory<Reject>>, code, value| {
        let mut request = command(code, 5);
        request.data[1] = value;
        runtime.dispatch_command(0, request).unwrap()
    };
    let mut point = [0; 48];
    point[..14].copy_from_slice(b"audio/tone.ogg");
    point[32..40].copy_from_slice(&3i64.to_le_bytes());
    point[40..].copy_from_slice(&99i64.to_le_bytes());
    runtime
        .dispatch_command_with_buffers(
            0,
            command(199, 0),
            &mut CommandBuffers {
                data1: Some(&mut point),
                ..Default::default()
            },
        )
        .unwrap();
    load(&mut runtime, &mut record(5, 0, 17, b"audio/tone.ogg")).unwrap();
    set(&mut runtime, 187, 47);
    runtime.host_mut().system_time_ms = 1234;
    set(&mut runtime, 183, 2);
    assert_eq!(audio(&runtime).channel(5).unwrap().started_tick(), 1234);
    assert_eq!(runtime.dispatch_command(0, command(186, 5)).unwrap(), 1);
    assert_eq!(
        read_channel(&mut runtime, 400).samples(),
        &expected.samples()[..400]
    );
    set(&mut runtime, 184, 0);
    runtime.update(100, 1).unwrap();
    assert_eq!(read_channel(&mut runtime, 50).samples(), &[0; 50]);
    assert_eq!(audio(&runtime).channel(5).unwrap().position_frames(), 400);
    assert_eq!(runtime.dispatch_command(0, command(186, 5)).unwrap(), 0);
    runtime.host_mut().system_time_ms = 2000;
    set(&mut runtime, 183, 1);
    assert_eq!(
        read_channel(&mut runtime, 979).samples(),
        &expected.samples()[400..]
    );
    assert_eq!(audio(&runtime).channel(5).unwrap().started_tick(), 1234);
    assert_eq!(runtime.dispatch_command(0, command(186, 5)).unwrap(), 0);
    assert_eq!(audio(&runtime).channel(5).unwrap().position_frames(), 0);
    set(&mut runtime, 183, 2);
    let samples = read_channel(&mut runtime, 1400);
    assert_eq!(&samples.samples()[..1379], expected.samples());
    assert_eq!(&samples.samples()[1379..], &expected.samples()[..21]);
    assert_eq!(audio(&runtime).channel(5).unwrap().remaining_plays(), 1);
    set(&mut runtime, 219, 1);
    let samples = read_channel(&mut runtime, 1362);
    assert_eq!(&samples.samples()[..1358], &expected.samples()[21..]);
    assert_eq!(&samples.samples()[1358..], [0; 4]);
    assert_eq!(audio(&runtime).channel(5).unwrap().completed_passes(), 1);
    set(&mut runtime, 183, 0);
    read_channel(&mut runtime, 100);
    set(&mut runtime, 183, 1);
    assert_eq!(
        read_channel(&mut runtime, 100).samples(),
        &expected.samples()[..100]
    );
    set(&mut runtime, 185, 0);
    assert_eq!(audio(&runtime).channel(5).unwrap().position_frames(), 0);
    assert_eq!(audio(&runtime).channel(5).unwrap().completed_passes(), 0);
    assert_eq!(audio(&runtime).channel(5).unwrap().volume(), Some(47));
    set(&mut runtime, 183, 1);
    assert!(
        runtime
            .object_mut(1)
            .unwrap()
            .audio_state_mut()
            .unwrap()
            .read_channel(5, usize::MAX)
            .is_err()
    );
    assert!(audio(&runtime).channel(5).unwrap().is_playing());
    assert_eq!(audio(&runtime).channel(5).unwrap().position_frames(), 0);
    load(&mut runtime, &mut record(5, 1, 17, b"audio/tone.ogg")).unwrap();
    assert!(
        runtime
            .dispatch_command(
                0,
                Command {
                    data: [5, 1, 0],
                    ..command(183, 5)
                }
            )
            .unwrap_err()
            .message
            .contains("淡变")
    );
    assert!(!audio(&runtime).channel(37).unwrap().is_playing());
    assert!(
        runtime
            .dispatch_command(
                0,
                Command {
                    data: [5, 100 | (10 << 16), 0],
                    ..command(189, 5)
                }
            )
            .unwrap_err()
            .message
            .contains("延迟释放")
    );
    set(&mut runtime, 185, 0);
    assert_eq!(runtime.dispatch_command(0, command(186, 5)).unwrap(), 0);
    set(&mut runtime, 182, 0);
    assert_eq!(audio(&runtime).channel(5).unwrap().started_tick(), u32::MAX);
    runtime.host_mut().system_time_ms = 15;
    set(&mut runtime, 218, 0);
    runtime.host_mut().system_time_ms = 16;
    set(&mut runtime, 218, 0);
    assert_eq!(audio(&runtime).channel(5).unwrap().started_tick(), 15);
    assert_eq!(audio(&runtime).channel(37).unwrap().started_tick(), 15);
}

#[test]
fn channel_properties_keep_resource_defaults_and_update_only_the_linked_target() {
    let (mut runtime, _) = setup();
    let set = |runtime: &mut Runtime<SoundFactory<Reject>>, code, channel, value: i32| {
        let mut request = command(code, channel);
        request.data[1] = value as u32;
        runtime.dispatch_command(0, request).unwrap()
    };
    set(&mut runtime, 187, 5, 47);
    assert_eq!(audio(&runtime).channel(5).unwrap().requested_volume(), 47.0);
    assert_eq!(runtime.dispatch_command(0, command(188, 5)).unwrap(), 0);
    load(&mut runtime, &mut record(5, 0, 17, b"audio/tone.ogg")).unwrap();
    assert_eq!(runtime.dispatch_command(0, command(188, 5)).unwrap(), 100);
    assert_eq!(audio(&runtime).channel(5).unwrap().requested_volume(), 47.0);
    set(&mut runtime, 187, 5, i32::MAX);
    set(&mut runtime, 190, 5, -131);
    assert_eq!(
        runtime.dispatch_command(0, command(188, 5)).unwrap(),
        i32::MAX as u32
    );
    assert_eq!(
        runtime.dispatch_command(0, command(191, 5)).unwrap(),
        (-100i32) as u32
    );
    assert_eq!(
        audio(&runtime).channel(5).unwrap().requested_volume(),
        2_147_483_648.0
    );
    runtime.host_mut().sound_stereo = Some(0);
    set(&mut runtime, 190, 5, 83);
    assert_eq!(
        runtime.dispatch_command(0, command(191, 5)).unwrap(),
        (-100i32) as u32
    );
    runtime.host_mut().sound_stereo = Some(u32::MAX);
    load(&mut runtime, &mut record(5, 1, 70, b"audio/stereo.ogg")).unwrap();
    set(&mut runtime, 187, 5, -17);
    set(&mut runtime, 190, 5, 131);
    for (code, expected) in [(188, (-17i32) as u32), (191, 100)] {
        for channel in [5, 37] {
            assert_eq!(
                runtime.dispatch_command(0, command(code, channel)).unwrap(),
                expected
            );
        }
    }
    assert_eq!(audio(&runtime).channel(5).unwrap().volume(), Some(i32::MAX));
    assert_eq!(audio(&runtime).channel(5).unwrap().pan(), Some(-100));
    runtime.dispatch_command(0, command(182, 5)).unwrap();
    set(&mut runtime, 187, 5, 16_777_217);
    assert_eq!(
        audio(&runtime).channel(37).unwrap().requested_volume(),
        16_777_216.0
    );
    assert_eq!(audio(&runtime).channel(37).unwrap().volume(), None);
    assert_eq!(runtime.dispatch_command(0, command(188, 5)).unwrap(), 0);

    load(&mut runtime, &mut record(5, 0, 17, b"audio/tone.ogg")).unwrap();
    let mut objects =
        katze_system2::runtime::fes::Objects::new(runtime.host().handle_allocator()).unwrap();
    let id = objects
        .define(&katze_system2::fes::ObjectDefinition {
            kind: "SOUND",
            name: "music".into(),
            banks: 1,
        })
        .unwrap()[0];
    let katze_system2::runtime::fes::ObjectState::Sound(sound) =
        &mut objects.get_mut(id).unwrap().state
    else {
        unreachable!()
    };
    sound.file = b"audio/tone.ogg".to_vec();
    sound.volume = 47;
    sound.pan = -43;
    sound
        .apply(&katze_system2::fes::ObjectAttributes {
            channel: Some(5),
            ..Default::default()
        })
        .unwrap();
    objects.set_sound_task(1);
    runtime.host_mut().sound_config_enabled = 1;
    runtime.host_mut().sound_config = Some(katze_system2::audio::SoundConfig {
        master_volume: Some(100),
        parts: vec![katze_system2::audio::SoundPart {
            file_prefix: b"audio/".to_vec(),
            volume: 65,
            mute: 0,
            ..Default::default()
        }],
        ..Default::default()
    });
    runtime.update_fes_object(0, &mut objects, id).unwrap();
    assert_eq!(runtime.dispatch_command(0, command(188, 5)).unwrap(), 30);
    assert_eq!(
        runtime.dispatch_command(0, command(191, 5)).unwrap(),
        (-43i32) as u32
    );
}

#[test]
fn vorbis_channels_keep_both_sources_during_replacement_and_release_the_linked_pair() {
    let (mut runtime, source) = setup();
    let mut loop_record = [0; 48];
    loop_record[..14].copy_from_slice(b"audio/tone.ogg");
    loop_record[32..40].copy_from_slice(&3i64.to_le_bytes());
    loop_record[40..48].copy_from_slice(&99i64.to_le_bytes());
    runtime
        .dispatch_command_with_buffers(
            0,
            command(199, 0),
            &mut CommandBuffers {
                data1: Some(&mut loop_record),
                ..Default::default()
            },
        )
        .unwrap();
    runtime.update(10, 1).unwrap();
    assert_eq!(audio(&runtime).channel_time(5).unwrap(), 10);
    let mut first = record(5, 0, 17, b"audio/tone.ogg");
    first[0x114..0x118].copy_from_slice(&7u32.to_le_bytes());
    assert_eq!(load(&mut runtime, &mut first).unwrap(), 1);
    let channel = audio(&runtime).channel(5).unwrap();
    assert_eq!(channel.file(), b"audio/tone.ogg");
    assert_eq!(channel.pcm().unwrap().frames(), 1379);
    assert_eq!(channel.pcm().unwrap().channels(), 1);
    assert_eq!(channel.load_parameters(), [0, 17]);
    assert_eq!(channel.load_options(), 7);
    assert_eq!(channel.loop_range(), [3, 99]);
    assert_eq!(audio(&runtime).channel_time(5).unwrap(), 0);
    assert_eq!(runtime.dispatch_command(0, command(201, 5)).unwrap(), 31);
    assert_eq!(
        source.borrow().events,
        [
            Event::Read(b"audio/tone.ogg".to_vec()),
            Event::Resolve(b"tag".to_vec(), b"tone".to_vec()),
            Event::Read(b"tag/tone.tag".to_vec()),
            Event::Resolve(b"sound".to_vec(), b"tone".to_vec()),
            Event::Read(b"sound/tone.plv".to_vec()),
            Event::Resolve(b"sound".to_vec(), b"tone".to_vec()),
            Event::Read(b"sound/tone.lpb".to_vec()),
            Event::Resolve(b"sound".to_vec(), b"tone".to_vec()),
            Event::Read(b"sound/tone.lv".to_vec()),
        ]
    );
    assert_eq!(
        load(&mut runtime, &mut record(5, 1, 70, b"audio/stereo.ogg")).unwrap(),
        1
    );
    let primary = audio(&runtime).channel(5).unwrap();
    let backup = audio(&runtime).channel(37).unwrap();
    assert_eq!(primary.pcm().unwrap().frames(), 1379);
    assert_eq!(primary.linked_channel(), Some(37));
    assert_eq!(primary.load_parameters(), [0, 17]);
    assert_eq!(backup.pcm().unwrap().frames(), 1903);
    assert_eq!(backup.pcm().unwrap().channels(), 2);
    assert_eq!(backup.load_parameters(), [1, 70]);
    assert_eq!(runtime.dispatch_command(0, command(201, 5)).unwrap(), 39);
    runtime.dispatch_command(0, command(182, 5)).unwrap();
    for index in [5, 37] {
        assert!(audio(&runtime).channel(index).unwrap().pcm().is_none());
        assert_eq!(audio(&runtime).channel_time(index).unwrap(), 0);
    }
    assert_eq!(
        audio(&runtime).channel(5).unwrap().linked_channel(),
        Some(37)
    );
    assert_eq!(audio(&runtime).channel(5).unwrap().loop_range(), [0, 99]);
    assert_eq!(runtime.dispatch_command(0, command(201, 5)).unwrap(), 0);
}

#[test]
fn load_failures_keep_completed_changes_and_diagnose_unimplemented_metadata() {
    let (mut runtime, source) = setup();
    let mut first = record(0, 0, 17, b"audio/tone.ogg");
    load(&mut runtime, &mut first).unwrap();
    source.borrow_mut().fail = true;
    assert_eq!(
        load(&mut runtime, &mut first).unwrap_err().message,
        "合成资源读取失败"
    );
    assert!(audio(&runtime).channel(0).unwrap().pcm().is_none());
    assert_eq!(
        audio(&runtime).channel(0).unwrap().load_parameters(),
        [0, 17]
    );
    assert_eq!(load(&mut runtime, &mut first).unwrap(), 1);
    assert!(load(&mut runtime, &mut record(0, 0, 3, b"audio/corrupt.ogg")).is_err());
    assert!(audio(&runtime).channel(0).unwrap().pcm().is_none());
    assert_eq!(
        load(&mut runtime, &mut record(0, 0, 3, b"audio/missing.ogg")).unwrap(),
        0
    );
    assert_eq!(
        audio(&runtime).channel(0).unwrap().load_parameters(),
        [0, 3]
    );
    source.borrow_mut().metadata = true;
    assert!(
        load(&mut runtime, &mut first)
            .unwrap_err()
            .message
            .contains("附加资源")
    );
    assert_eq!(
        audio(&runtime).channel(0).unwrap().pcm().unwrap().frames(),
        1379
    );
    assert_eq!(
        source.borrow().events.last(),
        Some(&Event::Read(b"tag/tone.tag".to_vec()))
    );
    assert!(load(&mut runtime, &mut first[..287]).is_err());
    assert!(load(&mut runtime, &mut record(32, -1, 0, b"audio/tone.ogg")).is_err());
    assert_eq!(runtime.dispatch_command(0, command(201, 64)).unwrap(), 0);
    runtime.dispatch_command(0, command(182, u32::MAX)).unwrap();
}
