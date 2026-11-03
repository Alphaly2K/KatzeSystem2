use katze_system2::{
    Error, Result,
    audio::SoundFactory,
    expression::State,
    kcs::ResourceServices,
    resource::ResourceMode,
    runtime::{BaseObject, ObjectFactory, Runtime, TaskObject, kind},
    task::{Command, CommandBuffers},
};
use std::{cell::RefCell, rc::Rc};

const MONO: &[u8] = include_bytes!("fixtures/audio/mono-44100.ogg");
const STEREO: &[u8] = include_bytes!("fixtures/audio/stereo-48000.ogg");

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
    bytes[0x114..0x118].copy_from_slice(&7u32.to_le_bytes());
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
    assert_eq!(
        load(&mut runtime, &mut record(5, 0, 17, b"audio/tone.ogg")).unwrap(),
        1
    );
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
