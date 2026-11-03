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
