use katze_system2::{
    Result,
    expression::State,
    graphics::{GraphicsFactory, Projection},
    kcs::{Program, ResourceServices, ScriptRandom},
    resource::ResourceMode,
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
    task::{Command, CommandBuffers},
};

fn plugin(name: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0; 512];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[64..68].copy_from_slice(b"PE\0\0");
    for (at, value) in [(68, 0x14cu16), (70, 1), (84, 112), (88, 0x10b)] {
        bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }
    for (at, value) in [
        (60, 64u32),
        (180, 1),
        (184, 0x1000),
        (212, 0x1000),
        (216, 256),
        (220, 256),
        (268, 0x1040),
        (272, 1),
        (276, 4),
        (284, 0x1054),
        (348, 0x1068),
        (352, 0x1070),
    ] {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes[320..320 + name.len()].copy_from_slice(name);
    bytes[360..366].copy_from_slice(&[0xb8, 1, 0, 2, 0, 0xc3]);
    if name == b"em_down.ppi" {
        bytes[361] = 2;
    }
    bytes[368..374].copy_from_slice(&[0xb8, 1, 0, 0, 0, 0xc3]);
    bytes
}

struct Files(Vec<u8>);
impl ResourceServices for Files {
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        assert_eq!(category, b"image");
        assert!(name.is_empty());
        Ok(Vec::new())
    }

    fn resolve_file(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        assert_eq!(category, b"ptclpi");
        Ok(name.to_vec())
    }
    fn read(&mut self, _: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        assert_eq!(mode, ResourceMode::File);
        Ok(Some(self.0.clone()))
    }
}

struct Programs(Program);
impl ProgramLoader for Programs {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        Ok(Some(self.0.clone()))
    }
}

fn runtime(bytes: Vec<u8>) -> Result<Runtime<GraphicsFactory<KcsFactory<Programs>>>> {
    let mut code = Vec::new();
    for value in [1u32, 0, 16, 8, 5000f32.to_bits(), 32] {
        code.extend([0x25, 3]);
        code.extend(value.to_le_bytes());
    }
    code.extend([0x2b, 3]);
    code.extend(693u32.to_le_bytes());
    code.push(3);
    code.extend(24u32.to_le_bytes());
    code.push(0x3b);
    let mut data = vec![0; 256];
    data[..10].copy_from_slice(b"effect.ppi");
    for (i, value) in [20f32, 0., 5., 10., 60., 8., 1.].into_iter().enumerate() {
        data[32 + i * 4..36 + i * 4].copy_from_slice(&value.to_le_bytes());
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
    runtime.host_mut().surface_size = [1280, 720];
    runtime.host_mut().random = ScriptRandom::seeded(17);
    runtime.set_resources(Files(bytes));
    runtime.create(0, kind::PARTICLE_3D, b"", b"")?;
    runtime.create(0, kind::ORDINARY, b"", b"")?;
    Ok(runtime)
}

fn command(code: u32) -> Command {
    Command {
        target: 1,
        sender: 2,
        code,
        data: [0; 3],
    }
}

fn assert_random(
    runtime: &mut Runtime<GraphicsFactory<KcsFactory<Programs>>>,
    expected: &ScriptRandom,
) {
    let mut actual = runtime.host_mut().random.clone();
    let mut expected = expected.clone();
    assert_eq!(actual.next_integer(), expected.next_integer());
    assert_eq!(actual.next_float(), expected.next_float());
}

#[test]
fn rain_shares_random_state_projects_motion_and_restarts_with_current_parameters() -> Result<()> {
    let mut runtime = runtime(plugin(b"rain.ppi"))?;
    let mut expected = ScriptRandom::seeded(17);
    runtime.update(1, 1)?;
    assert_eq!(runtime.dispatch_command(2, command(324))?, 0);
    assert!(runtime.object(1).unwrap().interpreter().is_none());
    assert_random(&mut runtime, &expected);
    for i in 0..8 {
        expected.next_integer();
        expected.next_integer();
        for _ in 0..4 + usize::from(i < 4) {
            expected.next_float();
        }
    }
    runtime.dispatch_command(2, command(321))?;
    assert_eq!(runtime.dispatch_command(2, command(324))?, 1);
    assert_random(&mut runtime, &expected);
    let view = Projection::default();
    runtime.update(2, 1)?;
    expected.next_integer();
    let first = runtime
        .drawables()
        .get(1)
        .unwrap()
        .particle_mesh()
        .unwrap()
        .projected_quad(0, &view)
        .unwrap();
    assert_eq!(first[0].color >> 24, 0);
    assert_ne!(first[0].position[1], first[1].position[1]);
    assert!(
        first
            .iter()
            .all(|v| v.rhw > 0. && v.uv.iter().all(|&n| (0.0..=1.0).contains(&n)))
    );
    runtime.update(3, 1)?;
    expected.next_integer();
    assert_random(&mut runtime, &expected);
    let mesh = runtime
        .drawables()
        .get(1)
        .unwrap()
        .particle_mesh()
        .unwrap()
        .clone();
    let second = mesh.projected_quad(0, &view).unwrap();
    assert!(second[0].position[1] > first[0].position[1]);
    assert!(second[0].color >> 24 >= 50);
    runtime.dispatch_command(2, command(322))?;
    runtime.update(4, 1)?;
    assert_random(&mut runtime, &expected);
    assert_eq!(runtime.drawables().get(1).unwrap().display(), 0);
    assert_eq!(
        runtime.drawables().get(1).unwrap().particle_mesh(),
        Some(&mesh)
    );
    let mut parameters = [0; 128];
    for (i, value) in [80f32, 0., 0., 0., 60., 8., 0.].into_iter().enumerate() {
        parameters[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    runtime.dispatch_command_with_buffers(
        2,
        command(325),
        &mut CommandBuffers {
            data1: Some(&mut parameters),
            ..Default::default()
        },
    )?;
    runtime.dispatch_command(2, command(321))?;
    for _ in 0..16 {
        expected.next_integer();
    }
    for _ in 0..32 {
        expected.next_float();
    }
    runtime.update(5, 1)?;
    expected.next_integer();
    let restarted = runtime
        .drawables()
        .get(1)
        .unwrap()
        .particle_mesh()
        .unwrap()
        .projected_quad(0, &view)
        .unwrap();
    assert_eq!(restarted[0].color >> 24, 0);
    assert_eq!(restarted[0].position[1], restarted[1].position[1]);
    runtime.dispatch_command(2, command(323))?;
    assert!(
        runtime
            .drawables()
            .get(1)
            .unwrap()
            .particle_mesh()
            .is_none()
    );
    assert_eq!(runtime.dispatch_command(2, command(324))?, 0);
    runtime.update(6, 1)?;
    assert_random(&mut runtime, &expected);
    Ok(())
}

#[test]
fn malformed_or_unsupported_plugin_metadata_is_rejected_before_start() -> Result<()> {
    let bytes = plugin(b"rain.ppi");
    for length in [0, 1, 63, 87, 199, 239, 295, 328, 351, 365, 373] {
        let mut runtime = runtime(bytes[..length].to_vec())?;
        assert!(runtime.update(1, 1).is_err());
        assert!(
            runtime
                .drawables()
                .get(1)
                .unwrap()
                .particle_mesh()
                .is_none()
        );
    }
    for at in [68, 88, 268, 272, 276, 284, 320, 361, 369] {
        let mut bytes = plugin(b"rain.ppi");
        bytes[at] = 0;
        assert!(runtime(bytes)?.update(1, 1).is_err());
    }
    for (name, version) in [(b"rain.ppi".as_slice(), 2), (b"em_down.ppi", 1)] {
        let mut bytes = plugin(name);
        bytes[361] = version;
        assert!(runtime(bytes)?.update(1, 1).is_err());
    }
    Ok(())
}

#[test]
fn notice_follows_source_positions_retains_missing_sources_and_keeps_random_state() -> Result<()> {
    use katze_system2::graphics::Drawable;

    let mut runtime = runtime(plugin(b"em_notice.ppi"))?;
    runtime.update(1, 1)?;
    let expected = ScriptRandom::seeded(17);
    let mut parameters = [0; 128];
    for (i, value) in [810f32, 181., 1., 50., 1., 1., 1., 1., 1.]
        .into_iter()
        .enumerate()
    {
        parameters[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    runtime.dispatch_command_with_buffers(
        2,
        command(325),
        &mut CommandBuffers {
            data1: Some(&mut parameters),
            ..Default::default()
        },
    )?;
    runtime.drawables_mut().insert(100, Drawable::default())?;
    runtime.drawables_mut().set_position(100, [640., 360., 0.]);
    let mut source = command(327);
    source.data[1] = 100;
    runtime.dispatch_command(2, source)?;
    runtime.dispatch_command(2, command(321))?;
    let view = Projection::default();
    runtime.update(2, 1)?;
    let quad = runtime
        .drawables()
        .get(1)
        .unwrap()
        .particle_mesh()
        .unwrap()
        .projected_quad(0, &view)
        .unwrap();
    assert_eq!(quad[0].position[..2], [874., 117.]);
    assert_eq!(quad[1].position[..2], [746., 117.]);
    assert_eq!(quad[0].color, u32::MAX);
    runtime.drawables_mut().set_position(100, [656., 352., 99.]);
    runtime.update(3, 1)?;
    let moved = runtime
        .drawables()
        .get(1)
        .unwrap()
        .particle_mesh()
        .unwrap()
        .projected_quad(0, &view)
        .unwrap();
    assert_eq!(moved[0].position[..2], [890., 109.]);
    runtime.drawables_mut().remove(100);
    runtime.update(4, 1)?;
    assert_eq!(
        runtime
            .drawables()
            .get(1)
            .unwrap()
            .particle_mesh()
            .unwrap()
            .projected_quad(0, &view)
            .unwrap(),
        moved
    );
    source.data[1] = 0;
    runtime.dispatch_command(2, source)?;
    runtime.update(5, 1)?;
    let unbound = runtime
        .drawables()
        .get(1)
        .unwrap()
        .particle_mesh()
        .unwrap()
        .projected_quad(0, &view)
        .unwrap();
    assert_eq!(unbound[0].position[..2], [234., -243.]);
    assert_eq!(unbound[0].color >> 24, 0);
    assert_random(&mut runtime, &expected);
    runtime.dispatch_command(2, command(322))?;
    runtime.update(6, 1)?;
    assert_eq!(runtime.dispatch_command(2, command(324))?, 0);
    assert_random(&mut runtime, &expected);
    runtime.dispatch_command(2, command(323))?;
    assert!(
        runtime
            .drawables()
            .get(1)
            .unwrap()
            .particle_mesh()
            .is_none()
    );
    Ok(())
}

#[test]
fn anger_shares_random_initialization_updates_fades_and_restarts() -> Result<()> {
    use katze_system2::graphics::Drawable;

    let mut runtime = runtime(plugin(b"em_oko.ppi"))?;
    runtime.update(1, 1)?;
    let mut expected = ScriptRandom::seeded(17);
    let mut parameters = [0; 128];
    for (i, value) in [901f32, 176., 86., 10., 15., 25.].into_iter().enumerate() {
        parameters[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    runtime.dispatch_command_with_buffers(
        2,
        command(325),
        &mut CommandBuffers {
            data1: Some(&mut parameters),
            ..Default::default()
        },
    )?;
    runtime.drawables_mut().insert(100, Drawable::default())?;
    runtime.drawables_mut().set_position(100, [640., 360., 0.]);
    let mut source = command(327);
    source.data[1] = 100;
    runtime.dispatch_command(2, source)?;
    runtime.dispatch_command(2, command(321))?;
    for _ in 0..64 {
        expected.next_float();
    }
    assert_random(&mut runtime, &expected);
    let view = Projection::default();
    runtime.update(2, 1)?;
    expected.next_integer();
    let first = runtime
        .drawables()
        .get(1)
        .unwrap()
        .particle_mesh()
        .unwrap()
        .projected_quad(0, &view)
        .unwrap();
    assert_eq!(first[0].color >> 24, 0);
    for frame in 3..=5 {
        runtime.update(frame, 1)?;
        expected.next_integer();
        assert_random(&mut runtime, &expected);
    }
    let mesh = runtime
        .drawables()
        .get(1)
        .unwrap()
        .particle_mesh()
        .unwrap()
        .clone();
    let moved = mesh.projected_quad(0, &view).unwrap();
    assert_ne!(first[0].position, moved[0].position);
    assert!(moved[0].color >> 24 > 0);
    assert_ne!(moved[0].position[1], moved[1].position[1]);
    runtime.dispatch_command(2, command(322))?;
    runtime.update(6, 1)?;
    assert_eq!(runtime.drawables().get(1).unwrap().display(), 0);
    assert_eq!(
        runtime.drawables().get(1).unwrap().particle_mesh(),
        Some(&mesh)
    );
    assert_random(&mut runtime, &expected);
    runtime.dispatch_command(2, command(321))?;
    for _ in 0..64 {
        expected.next_float();
    }
    runtime.update(7, 1)?;
    expected.next_integer();
    let restarted = runtime
        .drawables()
        .get(1)
        .unwrap()
        .particle_mesh()
        .unwrap()
        .projected_quad(0, &view)
        .unwrap();
    assert_eq!(restarted[0].color >> 24, 0);
    assert_ne!(restarted[0].position, first[0].position);
    assert_eq!(runtime.dispatch_command(2, command(324))?, 1);
    runtime.dispatch_command(2, command(323))?;
    runtime.update(8, 1)?;
    assert!(
        runtime
            .drawables()
            .get(1)
            .unwrap()
            .particle_mesh()
            .is_none()
    );
    assert_random(&mut runtime, &expected);
    Ok(())
}

#[test]
fn sigh_uses_mirrored_atlas_regions_drains_trails_and_restarts_without_random_draws() -> Result<()>
{
    use katze_system2::graphics::Drawable;

    let mut runtime = runtime(plugin(b"em_sigh.ppi"))?;
    let expected = ScriptRandom::seeded(17);
    runtime.update(1, 1)?;
    let mut parameters = [0; 128];
    for (i, value) in [810f32, 181., 1., 60., 5., 5.].into_iter().enumerate() {
        parameters[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    runtime.dispatch_command_with_buffers(
        2,
        command(325),
        &mut CommandBuffers {
            data1: Some(&mut parameters),
            ..Default::default()
        },
    )?;
    runtime.drawables_mut().insert(100, Drawable::default())?;
    runtime.drawables_mut().set_position(100, [640., 360., 0.]);
    let mut source = command(327);
    source.data[1] = 100;
    runtime.dispatch_command(2, source)?;
    runtime.dispatch_command(2, command(321))?;
    runtime.update(2, 1)?;
    let view = Projection::default();
    let mesh = runtime.drawables().get(1).unwrap().particle_mesh().unwrap();
    let head = mesh.projected_quad(0, &view).unwrap();
    let trail = mesh.projected_quad(1, &view).unwrap();
    assert_eq!(head[0].uv, [0.5, 0.]);
    assert_eq!(head[3].uv, [0., 0.5]);
    assert_eq!(trail[0].uv, [1., 0.]);
    assert_eq!(trail[3].uv, [0.5, 0.5]);
    assert_eq!(head[0].color >> 24, 0);
    assert_eq!(trail[0].color >> 24, 30);
    assert_ne!(head[0].rhw, trail[0].rhw);
    for frame in 3..=42 {
        runtime.update(frame, 1)?;
    }
    let mesh = runtime.drawables().get(1).unwrap().particle_mesh().unwrap();
    let head = mesh.projected_quad(0, &view).unwrap();
    assert_eq!(head[0].color >> 24, 255);
    for index in 1..8 {
        assert_eq!(mesh.projected_quad(index, &view).unwrap()[0].color >> 24, 0);
    }
    runtime.drawables_mut().set_position(100, [650., 380., 0.]);
    runtime.update(43, 1)?;
    let mesh = runtime
        .drawables()
        .get(1)
        .unwrap()
        .particle_mesh()
        .unwrap()
        .clone();
    assert_ne!(
        mesh.projected_quad(0, &view).unwrap()[0].position,
        head[0].position
    );
    runtime.dispatch_command(2, command(322))?;
    runtime.update(44, 1)?;
    assert_eq!(runtime.drawables().get(1).unwrap().display(), 0);
    assert_eq!(
        runtime.drawables().get(1).unwrap().particle_mesh(),
        Some(&mesh)
    );
    runtime.dispatch_command(2, command(321))?;
    runtime.update(45, 1)?;
    let mesh = runtime.drawables().get(1).unwrap().particle_mesh().unwrap();
    assert_eq!(mesh.projected_quad(0, &view).unwrap()[0].color >> 24, 0);
    assert_eq!(mesh.projected_quad(1, &view).unwrap()[0].color >> 24, 30);
    assert_eq!(runtime.dispatch_command(2, command(324))?, 1);
    runtime.dispatch_command(2, command(323))?;
    runtime.update(46, 1)?;
    assert!(
        runtime
            .drawables()
            .get(1)
            .unwrap()
            .particle_mesh()
            .is_none()
    );
    assert_random(&mut runtime, &expected);
    Ok(())
}

#[test]
fn down_uses_shared_random_atlas_regions_source_following_and_restart() -> Result<()> {
    use katze_system2::graphics::Drawable;

    let mut runtime = runtime(plugin(b"em_down.ppi"))?;
    runtime.update(1, 1)?;
    let mut expected = ScriptRandom::seeded(17);
    let mut parameters = [0; 128];
    for (i, value) in [1125f32, 19., 27., -60., 80., 13., 5., 80., 5., 5.]
        .into_iter()
        .enumerate()
    {
        parameters[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    runtime.dispatch_command_with_buffers(
        2,
        command(325),
        &mut CommandBuffers {
            data1: Some(&mut parameters),
            ..Default::default()
        },
    )?;
    runtime.drawables_mut().insert(100, Drawable::default())?;
    runtime.drawables_mut().set_position(100, [640., 360., 0.]);
    let mut source = command(327);
    source.data[1] = 100;
    runtime.dispatch_command(2, source)?;
    runtime.dispatch_command(2, command(321))?;
    for _ in 0..14 {
        expected.next_float();
    }
    assert_random(&mut runtime, &expected);
    let view = Projection::default();
    runtime.update(2, 1)?;
    let mesh = runtime.drawables().get(1).unwrap().particle_mesh().unwrap();
    let head = mesh.projected_quad(0, &view).unwrap();
    let line = mesh.projected_quad(1, &view).unwrap();
    assert_eq!([head[0].uv, head[3].uv], [[0.25, 0.], [1., 0.5]]);
    assert_eq!([line[0].uv, line[3].uv], [[0., 0.], [0.25, 1.]]);
    assert_eq!(head[0].color >> 24, 0);
    for frame in 3..=70 {
        runtime.update(frame, 1)?;
    }
    let mesh = runtime.drawables().get(1).unwrap().particle_mesh().unwrap();
    let head = mesh.projected_quad(0, &view).unwrap();
    assert!(head[0].color >> 24 > 0);
    assert_eq!(mesh.projected_quad(7, &view).unwrap()[0].color >> 24, 255);
    runtime.drawables_mut().set_position(100, [650., 380., 0.]);
    runtime.update(71, 1)?;
    let mesh = runtime
        .drawables()
        .get(1)
        .unwrap()
        .particle_mesh()
        .unwrap()
        .clone();
    let moved = mesh.projected_quad(0, &view).unwrap();
    assert_eq!(moved[0].position[0] - head[0].position[0], 10.);
    assert_eq!(moved[0].position[1] - head[0].position[1], 20.);
    runtime.dispatch_command(2, command(322))?;
    runtime.update(72, 1)?;
    assert_eq!(runtime.drawables().get(1).unwrap().display(), 0);
    assert_eq!(
        runtime.drawables().get(1).unwrap().particle_mesh(),
        Some(&mesh)
    );
    runtime.dispatch_command(2, command(321))?;
    for _ in 0..14 {
        expected.next_float();
    }
    runtime.update(73, 1)?;
    let mesh = runtime.drawables().get(1).unwrap().particle_mesh().unwrap();
    assert_eq!(mesh.projected_quad(0, &view).unwrap()[0].color >> 24, 0);
    assert_eq!(mesh.projected_quad(1, &view).unwrap()[0].color >> 24, 0);
    assert_eq!(runtime.dispatch_command(2, command(324))?, 1);
    assert_random(&mut runtime, &expected);
    runtime.dispatch_command(2, command(323))?;
    assert!(
        runtime
            .drawables()
            .get(1)
            .unwrap()
            .particle_mesh()
            .is_none()
    );
    Ok(())
}

#[test]
fn sweat_preserves_random_state_drains_trails_and_restarts_from_the_current_source() -> Result<()> {
    use katze_system2::graphics::Drawable;

    let mut runtime = runtime(plugin(b"em_sweat.ppi"))?;
    runtime.update(1, 1)?;
    let expected = ScriptRandom::seeded(17);
    let mut parameters = [0; 128];
    for (i, value) in [821f32, 166., 86., 20., 30.].into_iter().enumerate() {
        parameters[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    runtime.dispatch_command_with_buffers(
        2,
        command(325),
        &mut CommandBuffers {
            data1: Some(&mut parameters),
            ..Default::default()
        },
    )?;
    runtime.drawables_mut().insert(100, Drawable::default())?;
    runtime.drawables_mut().set_position(100, [640., 360., 0.]);
    let mut source = command(327);
    source.data[1] = 100;
    runtime.dispatch_command(2, source)?;
    runtime.dispatch_command(2, command(321))?;
    runtime.update(2, 1)?;
    let view = Projection::default();
    let mesh = runtime.drawables().get(1).unwrap().particle_mesh().unwrap();
    let first = mesh.projected_quad(0, &view).unwrap();
    assert_eq!(first[0].color >> 24, 0);
    assert_eq!(mesh.projected_quad(1, &view).unwrap()[0].color >> 24, 30);
    assert_eq!([first[0].uv, first[3].uv], [[0., 0.], [1., 1.]]);
    for frame in 3..=100 {
        runtime.update(frame, 1)?;
    }
    let mesh = runtime
        .drawables()
        .get(1)
        .unwrap()
        .particle_mesh()
        .unwrap()
        .clone();
    let head = mesh.projected_quad(0, &view).unwrap();
    assert_eq!(head[0].color >> 24, 255);
    assert_ne!(head[0].position[1], first[0].position[1]);
    for i in 1..8 {
        assert_eq!(mesh.projected_quad(i, &view).unwrap()[0].color >> 24, 0);
    }
    runtime.dispatch_command(2, command(322))?;
    runtime.update(101, 1)?;
    assert_eq!(runtime.drawables().get(1).unwrap().display(), 0);
    assert_eq!(
        runtime.drawables().get(1).unwrap().particle_mesh(),
        Some(&mesh)
    );
    runtime.drawables_mut().set_position(100, [650., 380., 0.]);
    runtime.dispatch_command(2, command(321))?;
    runtime.update(102, 1)?;
    let mesh = runtime.drawables().get(1).unwrap().particle_mesh().unwrap();
    let restarted = mesh.projected_quad(0, &view).unwrap();
    assert_eq!(restarted[0].color >> 24, 0);
    assert_eq!(restarted[0].position[0] - first[0].position[0], 10.);
    assert_eq!(restarted[0].position[1] - first[0].position[1], 20.);
    assert_eq!(runtime.dispatch_command(2, command(324))?, 1);
    assert_random(&mut runtime, &expected);
    runtime.dispatch_command(2, command(323))?;
    assert!(
        runtime
            .drawables()
            .get(1)
            .unwrap()
            .particle_mesh()
            .is_none()
    );
    Ok(())
}

#[test]
fn dis_shares_jitter_randomness_and_restores_mirrored_geometry_on_restart() -> Result<()> {
    use katze_system2::graphics::Drawable;

    let mut runtime = runtime(plugin(b"em_dis.ppi"))?;
    runtime.update(1, 1)?;
    let mut expected = ScriptRandom::seeded(17);
    let mut parameters = [0; 128];
    for (i, value) in [1161f32, 16., 1., 40., 5., 8., 1., 10.]
        .into_iter()
        .enumerate()
    {
        parameters[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    runtime.dispatch_command_with_buffers(
        2,
        command(325),
        &mut CommandBuffers {
            data1: Some(&mut parameters),
            ..Default::default()
        },
    )?;
    runtime.drawables_mut().insert(100, Drawable::default())?;
    runtime.drawables_mut().set_position(100, [640., 360., 0.]);
    let mut source = command(327);
    source.data[1] = 100;
    runtime.dispatch_command(2, source)?;
    runtime.dispatch_command(2, command(321))?;
    runtime.update(2, 1)?;
    let view = Projection::default();
    let mesh = runtime.drawables().get(1).unwrap().particle_mesh().unwrap();
    let first = mesh.projected_quad(0, &view).unwrap();
    assert_eq!([first[0].uv, first[3].uv], [[0.5, 0.], [0., 0.5]]);
    assert_eq!(first[0].color >> 24, 0);
    assert_random(&mut runtime, &expected);
    for frame in 3..=20 {
        runtime.update(frame, 1)?;
    }
    expected.next_integer();
    for _ in 0..4 {
        expected.next_float();
    }
    assert_random(&mut runtime, &expected);
    let mesh = runtime
        .drawables()
        .get(1)
        .unwrap()
        .particle_mesh()
        .unwrap()
        .clone();
    assert_eq!(mesh.projected_quad(0, &view).unwrap()[0].color >> 24, 255);
    let jitter = mesh.projected_quad(1, &view).unwrap();
    assert_ne!(jitter[0].position[1], jitter[1].position[1]);
    runtime.dispatch_command(2, command(322))?;
    runtime.update(21, 1)?;
    assert_eq!(runtime.drawables().get(1).unwrap().display(), 0);
    assert_eq!(
        runtime.drawables().get(1).unwrap().particle_mesh(),
        Some(&mesh)
    );
    runtime.drawables_mut().set_position(100, [650., 380., 0.]);
    runtime.dispatch_command(2, command(321))?;
    runtime.update(22, 1)?;
    let mesh = runtime.drawables().get(1).unwrap().particle_mesh().unwrap();
    let restarted = mesh.projected_quad(0, &view).unwrap();
    assert_eq!(
        [restarted[0].uv, restarted[3].uv],
        [first[0].uv, first[3].uv]
    );
    assert_eq!(restarted[0].color >> 24, 0);
    assert_eq!(restarted[0].position[0] - first[0].position[0], 10.);
    assert_eq!(restarted[0].position[1] - first[0].position[1], 20.);
    assert_random(&mut runtime, &expected);
    runtime.dispatch_command(2, command(323))?;
    assert!(
        runtime
            .drawables()
            .get(1)
            .unwrap()
            .particle_mesh()
            .is_none()
    );
    Ok(())
}

#[test]
fn common_moves_and_holds_without_random_draws_and_restarts_at_the_source() -> Result<()> {
    use katze_system2::graphics::Drawable;

    let mut runtime = runtime(plugin(b"em_common.ppi"))?;
    runtime.update(1, 1)?;
    let expected = ScriptRandom::seeded(17);
    let mut parameters = [0; 128];
    for (i, value) in [782f32, 169., 1., 60., 5., 8.].into_iter().enumerate() {
        parameters[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    runtime.dispatch_command_with_buffers(
        2,
        command(325),
        &mut CommandBuffers {
            data1: Some(&mut parameters),
            ..Default::default()
        },
    )?;
    runtime.drawables_mut().insert(100, Drawable::default())?;
    runtime.drawables_mut().set_position(100, [640., 360., 0.]);
    let mut source = command(327);
    source.data[1] = 100;
    runtime.dispatch_command(2, source)?;
    runtime.dispatch_command(2, command(321))?;
    runtime.update(2, 1)?;
    let view = Projection::default();
    let first = runtime
        .drawables()
        .get(1)
        .unwrap()
        .particle_mesh()
        .unwrap()
        .projected_quad(0, &view)
        .unwrap();
    assert_eq!(first[0].color >> 24, 0);
    assert_eq!([first[0].uv, first[3].uv], [[0., 0.], [1., 1.]]);
    for frame in 3..=30 {
        runtime.update(frame, 1)?;
    }
    let mesh = runtime
        .drawables()
        .get(1)
        .unwrap()
        .particle_mesh()
        .unwrap()
        .clone();
    let held = mesh.projected_quad(0, &view).unwrap();
    assert_eq!(held[0].color >> 24, 255);
    assert_ne!(held[0].position, first[0].position);
    runtime.dispatch_command(2, command(322))?;
    runtime.update(31, 1)?;
    assert_eq!(runtime.drawables().get(1).unwrap().display(), 0);
    assert_eq!(
        runtime.drawables().get(1).unwrap().particle_mesh(),
        Some(&mesh)
    );
    runtime.drawables_mut().set_position(100, [650., 380., 0.]);
    runtime.dispatch_command(2, command(321))?;
    runtime.update(32, 1)?;
    let restarted = runtime
        .drawables()
        .get(1)
        .unwrap()
        .particle_mesh()
        .unwrap()
        .projected_quad(0, &view)
        .unwrap();
    assert_eq!(restarted[0].color >> 24, 0);
    assert_eq!(restarted[0].position[0] - first[0].position[0], 10.);
    assert_eq!(restarted[0].position[1] - first[0].position[1], 20.);
    assert_random(&mut runtime, &expected);
    runtime.dispatch_command(2, command(323))?;
    assert!(
        runtime
            .drawables()
            .get(1)
            .unwrap()
            .particle_mesh()
            .is_none()
    );
    Ok(())
}
