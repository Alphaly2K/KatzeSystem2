use katze_system2::{
    Result,
    expression::State,
    graphics::{
        Drawable, Drawables, GraphicsFactory, ParticleMesh, Projection, Surface, TextureFilter,
        TextureSource,
    },
    kcs::{ParticleHost, Program, ResourceServices, ScriptRandom},
    resource::ResourceMode,
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
    task::{Command, CommandBuffers},
};
use std::{io::Write, sync::Arc};

fn ins(code: &mut Vec<u8>, opcode: u8, args: &[u32]) {
    code.push(opcode);
    for value in args {
        code.push(3);
        code.extend(value.to_le_bytes());
    }
}

fn native(code: &mut Vec<u8>, index: u32, args: &[u32]) {
    for &value in args {
        ins(code, 0x25, &[value]);
    }
    ins(code, 0x2b, &[index, args.len() as u32 * 4]);
}

fn encoded(code: &[u8], control: u32) -> Vec<u8> {
    let length = code.len() as u32;
    let mut bytes = b"KCS\0".to_vec();
    for value in [
        2,
        control,
        52,
        length,
        52 + length,
        0,
        0,
        256,
        64,
        52 + length,
        0,
        length,
    ] {
        bytes.extend(value.to_le_bytes());
    }
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), Default::default());
    encoder.write_all(code).unwrap();
    bytes.extend(encoder.finish().unwrap());
    bytes
}

struct Programs(Program);
impl ProgramLoader for Programs {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        Ok(Some(self.0.clone()))
    }
}

struct Files(Vec<u8>);
impl ResourceServices for Files {
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        assert!(matches!(category, b"ptcl" | b"image"));
        Ok([category, b"/", name].concat())
    }
    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        assert_eq!(mode, ResourceMode::LooseFirst);
        match name {
            b"ptcl/test" => Ok(Some(self.0.clone())),
            b"image/sprite" => Ok(None),
            b"ptcl/missing" => Ok(None),
            _ => panic!("unexpected synthetic resource"),
        }
    }
}

#[test]
fn particle_runtime_loads_native_packets_resumes_and_shares_random_state() -> Result<()> {
    let mut particle_code = Vec::new();
    native(&mut particle_code, 33, &[]);
    ins(&mut particle_code, 0x26, &[0]);
    native(&mut particle_code, 35, &[16]);
    particle_code.push(0x3b);
    for (budget, control) in [1, 4096]
        .into_iter()
        .flat_map(|budget| [0, 1, 2].map(|control| (budget, control)))
    {
        let mut code = Vec::new();
        for id in [1, 2] {
            native(&mut code, 271, &[id, 0, 8, 1, 5000f32.to_bits(), 32]);
            ins(&mut code, 0x26, &[200 + id * 4]);
            native(&mut code, 272, &[id]);
        }
        code.push(0x3b);
        let mut data = vec![0; 256];
        data[..5].copy_from_slice(b"test\0");
        data[8..15].copy_from_slice(b"sprite\0");
        for i in 0..32 {
            data[32 + i * 4..36 + i * 4]
                .copy_from_slice(&(0xfed0_0000u32 + i as u32).to_le_bytes());
        }
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            GraphicsFactory {
                fallback: KcsFactory::new(Programs(Program::from_code(&code, data, 128)?)),
            },
        );
        runtime.set_budget(budget, 1 << 20);
        runtime.host_mut().surface_size = [1279, 719];
        runtime.host_mut().random = ScriptRandom::seeded(17);
        runtime.set_resources(Files(encoded(&particle_code, control)));
        for _ in 0..2 {
            runtime.create(0, kind::PARTICLE_3D, b"", b"context")?;
        }
        let caller = runtime.create(0, kind::ORDINARY, b"start", b"")?;
        runtime.update(1, 1)?;
        for id in [1, 2] {
            assert_eq!(
                runtime
                    .object(caller)
                    .unwrap()
                    .interpreter()
                    .unwrap()
                    .read(200 + id * 4, 4)?,
                1u32.to_le_bytes()
            );
            assert_eq!(runtime.drawables().get(id).unwrap().display(), 1);
            assert_eq!(
                runtime.object(id).unwrap().creation_context(),
                Some(&b"context"[..])
            );
        }
        let mut expected = ScriptRandom::seeded(17);
        for time in [2, 5] {
            runtime.update(time, 1)?;
            for id in [1, 2] {
                let vm = runtime.object(id).unwrap().interpreter().unwrap();
                assert_eq!(vm.read(0, 4)?, expected.next_integer().to_le_bytes());
                assert_eq!(vm.read(16, 4)?, 0xfed0_0000u32.to_le_bytes());
                assert_eq!(vm.read(140, 4)?, 0xfed0_001fu32.to_le_bytes());
            }
        }
        let command = |code| Command {
            target: 1,
            sender: caller,
            code,
            data: [0; 3],
        };
        assert_eq!(runtime.dispatch_command(caller, command(324))?, 1);
        runtime.dispatch_command(caller, command(322))?;
        assert_eq!(runtime.dispatch_command(caller, command(324))?, 0);
        assert_eq!(runtime.drawables().get(1).unwrap().display(), 0);
        runtime.dispatch_command(caller, command(321))?;
        assert_eq!(runtime.drawables().get(1).unwrap().display(), 1);
        let mut plugin = [0; 0x290];
        plugin[0] = 1;
        plugin[4..14].copy_from_slice(b"ptcl/test\0");
        assert!(
            runtime
                .dispatch_command_with_buffers(
                    caller,
                    command(320),
                    &mut CommandBuffers {
                        data1: Some(&mut plugin),
                        ..Default::default()
                    }
                )
                .is_err()
        );
        plugin[4..132].fill(0);
        plugin[4..17].copy_from_slice(b"ptcl/missing\0");
        runtime.dispatch_command_with_buffers(
            caller,
            command(320),
            &mut CommandBuffers {
                data1: Some(&mut plugin),
                ..Default::default()
            },
        )?;
        assert_eq!(runtime.dispatch_command(caller, command(324))?, 0);
        runtime.dispatch_command(caller, command(323))?;
        assert!(runtime.object(1).unwrap().interpreter().is_none());
        assert!(
            runtime
                .drawables()
                .get(1)
                .unwrap()
                .particle_mesh()
                .is_none()
        );
        assert_eq!(
            runtime.host_mut().random.next_integer(),
            expected.next_integer()
        );
    }
    Ok(())
}

#[test]
fn particle_mesh_projects_offsets_and_preserves_texture_lifetime_and_quad_flags() -> Result<()> {
    let mut host = ParticleHost {
        surface_size: [1279, 719],
        depth_offset: 5000.,
        ..Default::default()
    };
    host.reset_quads(2, [8., 4.])?;
    host.quad_flags[1] = 0;
    for vertex in &mut host.quads[0] {
        vertex.position[0] += 100.;
        vertex.position[1] += 80.;
    }
    host.offsets[0] = [2., 3., 0., 0.];
    let texture = Arc::new(Surface::from_bgra([1, 1], vec![3, 2, 1, 255])?);
    let mut mesh = ParticleMesh::new(&host, Some(texture.clone()), 59000.)?;
    mesh.set_opacity(128);
    let quad = mesh.projected_quad(0, &Projection::default()).unwrap();
    assert_eq!(quad[0].position[..2], [98., 77.]);
    assert_eq!(quad[0].rhw, 0.001);
    assert_eq!(quad[0].color, 0x7fff_ffff);
    let mut drawings = Drawables::default();
    drawings.insert(1, Drawable::from_particle_mesh(mesh))?;
    drawings.set_display(1, 1);
    drop(texture);
    let frame = drawings.compose(
        &Projection::default(),
        std::array::from_fn(|i| if i % 5 == 0 { 1. } else { 0. }),
        TextureFilter::Point,
        None,
    )?;
    assert_eq!(frame.commands.len(), 2);
    let TextureSource::Image(image) = &frame.commands[0].texture.as_ref().unwrap().source else {
        panic!("expected image source")
    };
    assert_eq!(image.pixels(), [3, 2, 1, 255]);
    assert_eq!(frame.commands[0].vertices[0].position, quad[0].position);
    assert_eq!(frame.commands[1].vertices[0].position, quad[2].position);
    drawings.remove(1);
    assert_eq!(image.pixels(), [3, 2, 1, 255]);
    Ok(())
}
