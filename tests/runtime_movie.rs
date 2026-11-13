use katze_system2::{
    Result,
    audio::SoundFactory,
    expression::State,
    graphics::{GraphicsBackend, Projection, RenderTarget, SoftwareBackend, TextureFilter},
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

fn setup(code: Vec<u8>, budget: usize, name: &[u8]) -> Runtime<SoundFactory<KcsFactory<Loader>>> {
    let mut data = vec![0; 256];
    data[64..64 + name.len()].copy_from_slice(name);
    data[128..132].copy_from_slice(&7u32.to_le_bytes());
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(RootObject::new(u32::MAX, 1)),
        State::new(0),
        SoundFactory {
            enabled: true,
            fallback: KcsFactory::new(Loader(Program::from_code(&code, data, 128).unwrap())),
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

fn word(runtime: &Runtime<SoundFactory<KcsFactory<Loader>>>, address: u32) -> u32 {
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
