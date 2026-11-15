use katze_system2::{
    Result,
    audio::SoundFactory,
    expression::State,
    graphics::{
        GraphicsBackend, GraphicsFactory, Projection, RenderTarget, SoftwareBackend, TextureFilter,
    },
    kcs::{Program, ResourceServices},
    movie::Decoder,
    resource::ResourceMode,
    runtime::{KcsFactory, ProgramLoader, RootObject, Runtime, kind},
};
use std::sync::Arc;

struct Loader(Program);
impl ProgramLoader for Loader {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        Ok(Some(self.0.clone()))
    }
}

struct Resources;
impl ResourceServices for Resources {
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        assert_eq!(category, b"movie");
        Ok(name.to_vec())
    }
    fn read(&mut self, _: &[u8], _: ResourceMode) -> Result<Option<Vec<u8>>> {
        panic!("movie must open a shared source")
    }
    fn open(
        &mut self,
        name: &[u8],
        mode: ResourceMode,
    ) -> Result<Option<katze_system2::io::SharedSource>> {
        assert_eq!(mode, ResourceMode::LooseFirst);
        Ok((name == b"clip_7.mpg")
            .then(|| Arc::new(include_bytes!("fixtures/movie/clip.mpg").to_vec()) as _))
    }
}

fn instruction(code: &mut Vec<u8>, opcode: u8, words: &[u32]) {
    code.push(opcode);
    for word in words {
        code.push(3);
        code.extend(word.to_le_bytes());
    }
}

fn call(code: &mut Vec<u8>, index: u32, args: &[u32], handle: bool) {
    if handle {
        code.extend([0x25, 0x13]);
        code.extend(0u32.to_le_bytes());
    }
    for &arg in args {
        instruction(code, 0x25, &[arg]);
    }
    instruction(
        code,
        0x2b,
        &[index, (args.len() as u32 + u32::from(handle)) * 4],
    );
}

type MovieRuntime = Runtime<SoundFactory<GraphicsFactory<KcsFactory<Loader>>>>;

fn setup(code: Vec<u8>, budget: usize, name: &[u8]) -> MovieRuntime {
    let mut data = vec![0; 256];
    data[64..64 + name.len()].copy_from_slice(name);
    data[128..132].copy_from_slice(&7u32.to_le_bytes());
    data[192..197].copy_from_slice(b"play\0");
    data[224..233].copy_from_slice(b"%u[G128]\0");
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(RootObject::new(u32::MAX, 1)),
        State::new(0),
        SoundFactory {
            enabled: true,
            fallback: GraphicsFactory {
                fallback: KcsFactory::new(Loader(Program::from_code(&code, data, 128).unwrap())),
            },
        },
    );
    runtime.set_budget(budget, 1 << 20);
    runtime.host_mut().surface_size = [32; 2];
    runtime.set_resources(Resources);
    runtime.drawing_mut().target = Some(RenderTarget::new([32; 2]).unwrap());
    runtime.drawing_mut().enabled = true;
    runtime.set_frame_view(Projection::default(), [0.; 16], TextureFilter::Point);
    assert_eq!(runtime.create(0, kind::SOUND, b"sound", b"").unwrap(), 1);
    assert_eq!(
        runtime.create(0, kind::ORDINARY, b"caller", b"").unwrap(),
        2
    );
    runtime
}

fn word(runtime: &MovieRuntime, address: u32) -> u32 {
    u32::from_le_bytes(
        runtime
            .object(2)
            .unwrap()
            .interpreter()
            .unwrap()
            .read(address, 4)
            .unwrap()
            .try_into()
            .unwrap(),
    )
}

#[test]
fn plane_movie_loads_without_exclusive_playback_or_audio_and_keeps_native_return_slot() {
    let mut code = Vec::new();
    instruction(&mut code, 0x25, &[0xdead_beef]);
    call(&mut code, 701, &[0, u32::MAX], false);
    instruction(&mut code, 0x26, &[0]);
    instruction(&mut code, 0x25, &[0xdead_beef]);
    call(&mut code, 701, &[3, 64], false);
    call(&mut code, 654, &[3, 192, 224], false);
    instruction(&mut code, 0x26, &[4]);
    instruction(&mut code, 0x3b, &[]);
    let mut runtime = setup(code, 4096, b"clip_%u[G128].mpg\0");
    assert_eq!(runtime.create(0, kind::PLANE, b"layer", b"").unwrap(), 3);
    runtime.update(0, 1).unwrap();
    assert_eq!(word(&runtime, 0), 0xdead_beef);
    assert_eq!(word(&runtime, 4), 0xdead_beef);
    assert!(runtime.drawing().enabled);
    let movie = runtime.drawables().get(3).unwrap();
    assert_eq!(movie.bounds(), [[0.; 3], [32., 32., 0.]]);
    assert!(movie.triangles().is_empty());
    assert!(runtime.compose_frame().unwrap().commands.is_empty());
    let mut audio = [1.; 2048];
    runtime
        .object_mut(1)
        .unwrap()
        .audio_state_mut()
        .unwrap()
        .mix_stereo(48000, &mut audio)
        .unwrap();
    assert!(audio.iter().all(|&sample| sample == 0.));
    runtime.destroy(3).unwrap();
    assert!(runtime.drawables().get(3).is_none());

    let mut code = Vec::new();
    call(&mut code, 654, &[0, 192, u32::MAX], false);
    let mut runtime = setup(code, 4096, b"unused\0");
    assert!(runtime.update(0, 1).is_err());
}

#[test]
fn movie_natives_play_present_finish_and_release_through_runtime_services() {
    for budget in [1, 4096] {
        let mut code = Vec::new();
        call(&mut code, 265, &[64], false);
        instruction(&mut code, 0x26, &[0]);
        for (index, args, address) in [(266, vec![1], 4), (998, vec![], 8), (269, vec![], 12)] {
            call(&mut code, index, &args, true);
            instruction(&mut code, 0x26, &[address]);
        }
        call(&mut code, 788, &[8, 8, 16, 16], true);
        call(&mut code, 65, &[], false);
        call(&mut code, 269, &[], true);
        instruction(&mut code, 0x26, &[16]);
        instruction(&mut code, 0x25, &[0xdead_beef]);
        call(&mut code, 270, &[], true);
        call(&mut code, 997, &[1], true);
        call(&mut code, 65, &[], false);
        call(&mut code, 997, &[0], true);
        call(&mut code, 268, &[], true);
        instruction(&mut code, 0x26, &[20]);
        instruction(&mut code, 0x26, &[24]);
        instruction(&mut code, 0x3b, &[]);
        let mut runtime = setup(code, budget, b"clip_%u[G128].mpg\0");
        runtime.update(0, 1).unwrap();
        assert_ne!(word(&runtime, 0), 0);
        assert_eq!(word(&runtime, 4), 1);
        assert_eq!(word(&runtime, 12), 1);
        assert!(!runtime.drawing().enabled);
        let decoder =
            Decoder::new(Arc::new(include_bytes!("fixtures/movie/clip.mpg").to_vec())).unwrap();
        assert_eq!(
            word(&runtime, 8),
            (decoder.metadata().duration * 1000.) as u32
        );
        let frame = runtime.compose_frame().unwrap();
        assert_eq!(frame.commands.len(), 4);
        let mut software = RenderTarget::new([32; 2]).unwrap();
        assert!(
            runtime
                .render_frame(
                    &mut software,
                    &Projection::default(),
                    [0.; 16],
                    TextureFilter::Point
                )
                .unwrap()
        );
        let composed = SoftwareBackend
            .render(
                &frame,
                runtime.drawing_mut().target.as_mut().unwrap(),
                [32; 2],
            )
            .unwrap();
        assert_eq!(software.tiles(), composed.tiles());
        let surface = software.tiles()[0].as_ref().unwrap();
        assert_eq!(surface.pixel(0, 0), Some(0xff00_0000));
        assert_ne!(surface.pixel(16, 16), Some(0xff00_0000));
        let mut audio = vec![0.; 2 * 9000];
        runtime
            .object_mut(1)
            .unwrap()
            .audio_state_mut()
            .unwrap()
            .mix_stereo(48000, &mut audio)
            .unwrap();
        assert!(audio.iter().any(|&sample| sample.abs() > 0.01));
        runtime.update(1, 1).unwrap();
        assert_eq!(word(&runtime, 16), 0);
        assert!(runtime.drawing().enabled);
        assert!(runtime.compose_frame().unwrap().commands.is_empty());
        runtime.update(2, 1).unwrap();
        assert_eq!(word(&runtime, 20), 1);
        assert_eq!(word(&runtime, 24), 0xdead_beef);
        assert!(runtime.drawing().enabled);
        audio.fill(1.);
        runtime
            .object_mut(1)
            .unwrap()
            .audio_state_mut()
            .unwrap()
            .mix_stereo(48000, &mut audio)
            .unwrap();
        assert!(audio.iter().all(|&sample| sample == 0.));
    }
}

#[test]
fn missing_and_disabled_movies_preserve_the_return_protocol_and_restore_drawing() {
    for disabled in [false, true] {
        let mut code = Vec::new();
        call(
            &mut code,
            265,
            &[if disabled { u32::MAX } else { 64 }],
            false,
        );
        instruction(&mut code, 0x26, &[0]);
        instruction(&mut code, 0x25, &[0xdead_beef]);
        for (index, address) in [(267, 8), (268, 12), (269, 16), (998, 20)] {
            call(&mut code, index, &[0], false);
            instruction(&mut code, 0x26, &[address]);
        }
        call(&mut code, 270, &[0], false);
        instruction(&mut code, 0x26, &[4]);
        instruction(&mut code, 0x3b, &[]);
        let mut runtime = setup(code, 1, b"missing.mpg\0");
        if disabled {
            runtime.host_mut().platform_settings =
                Some(katze_system2::settings::PlatformSettings {
                    etc_disablemovie: 1,
                    ..Default::default()
                });
        }
        runtime.update(0, 1).unwrap();
        assert_eq!(word(&runtime, 0), 0);
        assert_eq!(word(&runtime, 4), 0xdead_beef);
        assert!(runtime.drawing().enabled);
    }
}

#[test]
fn plane_movie_sources_release_on_replacement_failure_and_task_cleanup() {
    use katze_system2::{
        graphics::{Drawable, Drawables, PlaneObject},
        kcs::{Host, Interpreter, NativeReturn},
        runtime::TaskObject,
        task::{Command, CommandBuffers},
    };
    struct HostState {
        drawings: Drawables,
        source: Arc<Vec<u8>>,
        enabled: bool,
        opens: usize,
    }
    impl ResourceServices for HostState {
        fn read(&mut self, _: &[u8], _: ResourceMode) -> Result<Option<Vec<u8>>> {
            Ok(None)
        }
        fn open(
            &mut self,
            name: &[u8],
            mode: ResourceMode,
        ) -> Result<Option<katze_system2::io::SharedSource>> {
            assert_eq!(mode, ResourceMode::LooseFirst);
            self.opens += 1;
            Ok((name == b"clip.mpg").then(|| self.source.clone() as _))
        }
    }
    impl Host for HostState {
        fn call(&mut self, _: u32, _: &[u8], _: &mut Interpreter) -> Result<NativeReturn> {
            unreachable!()
        }
        fn resource_services(&mut self) -> Option<&mut dyn ResourceServices> {
            Some(self)
        }
        fn movie_enabled(&self) -> bool {
            self.enabled
        }
        fn drawables(&mut self) -> Option<&mut Drawables> {
            Some(&mut self.drawings)
        }
        fn canvas_layout(&self) -> Result<Option<katze_system2::graphics::CanvasLayout>> {
            katze_system2::graphics::CanvasLayout::new([32; 2], [1; 2], [32; 2]).map(Some)
        }
    }
    let mut host = HostState {
        drawings: Drawables::default(),
        source: Arc::new(include_bytes!("fixtures/movie/clip.mpg").to_vec()),
        enabled: true,
        opens: 0,
    };
    let mut plane = PlaneObject::new(7);
    let send = |plane: &mut PlaneObject, host: &mut HostState, code, value, record: &mut [u8]| {
        let mut output = 0xdead_beef;
        plane
            .dispatch_command_with_buffers(
                kind::PLANE,
                &Command {
                    target: 7,
                    sender: 19,
                    code,
                    data: [0, value, 0],
                },
                &mut CommandBuffers {
                    data1: Some(record),
                    ..Default::default()
                },
                &mut output,
                host,
                4096,
            )
            .unwrap();
        assert_eq!(output, 0xdead_beef);
    };
    let record = |name: &[u8]| {
        let mut bytes: Vec<_> = [-5.25f32, 13.5, 777.]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        bytes.extend(name);
        bytes.push(0);
        bytes
    };
    for clear in [41, 42, 45, 68, 70, 83, 0] {
        send(&mut plane, &mut host, 43, 0, &mut record(b"clip.mpg"));
        assert_eq!(Arc::strong_count(&host.source), 2);
        assert_eq!(plane.size(), [32.; 2]);
        assert_eq!(plane.offset(), [0.; 2]);
        assert_eq!(plane.shape_kind(), 3);
        assert_eq!(
            host.drawings.get(7).unwrap().bounds(),
            [[-5.25, 13.5, 0.], [26.75, 45.5, 0.]]
        );
        if clear == 0 {
            plane.prepare_release(&mut host, 4096).unwrap();
        } else {
            send(&mut plane, &mut host, clear, 0, &mut [0; 32]);
        }
        assert_eq!(Arc::strong_count(&host.source), 1);
    }
    for flags in [0, 1, 2, u32::MAX] {
        send(&mut plane, &mut host, 43, 0, &mut record(b"clip.mpg"));
        send(&mut plane, &mut host, 84, flags, &mut []);
        let previous = host.opens;
        send(&mut plane, &mut host, 43, 0, &mut record(b"missing"));
        assert_eq!(host.opens - previous, usize::from(flags == 1));
        assert!(host.drawings.get(7).is_none());
        assert_eq!(Arc::strong_count(&host.source), 1);
        send(&mut plane, &mut host, 84, 1, &mut []);
    }
    host.enabled = false;
    host.drawings.insert(7, Drawable::default()).unwrap();
    let previous = host.opens;
    send(&mut plane, &mut host, 43, 0, &mut record(b"clip.mpg"));
    assert_eq!(host.opens, previous);
    assert!(host.drawings.get(7).is_none());
}
