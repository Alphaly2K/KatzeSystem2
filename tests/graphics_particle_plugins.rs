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
    if name == b"rising3.ppi" {
        bytes[361] = 0;
    }
    if matches!(
        name,
        b"em_down.ppi"
            | b"em_silent.ppi"
            | b"em_kyupin.ppi"
            | b"energy.ppi"
            | b"digi.ppi"
            | b"digiring.ppi"
            | b"em_shy.ppi"
            | b"thunder.ppi"
            | b"protect.ppi"
    ) {
        bytes[361] = 2;
    }
    bytes[368..374].copy_from_slice(&[0xb8, 1, 0, 0, 0, 0xc3]);
    if name == b"spark.ppi" {
        bytes[340..344].copy_from_slice(&0x1090u32.to_le_bytes());
        bytes[400..403].copy_from_slice(&[0x55, 0x8b, 0xec]);
        bytes[435] = 0x68;
        bytes[436..440].copy_from_slice(&0x324u32.to_le_bytes());
    }
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
    runtime_with_count(bytes, 8)
}

fn runtime_with_count(
    bytes: Vec<u8>,
    count: u32,
) -> Result<Runtime<GraphicsFactory<KcsFactory<Programs>>>> {
    let mut code = Vec::new();
    for value in [1u32, 0, 16, count, 5000f32.to_bits(), 32] {
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
fn invalid_plugin_metadata_and_layout_are_rejected_before_start() -> Result<()> {
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
    for (name, version) in [
        (b"rain.ppi".as_slice(), 2),
        (b"em_down.ppi", 1),
        (b"em_silent.ppi", 1),
    ] {
        let mut bytes = plugin(name);
        bytes[361] = version;
        assert!(runtime(bytes)?.update(1, 1).is_err());
    }
    for count in [0, 5] {
        let mut runtime = runtime_with_count(plugin(b"em_silent.ppi"), count)?;
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
fn deterministic_emotions_preserve_randomness_task_controls_and_source_offsets() -> Result<()> {
    use katze_system2::graphics::Drawable;

    for (name, values, count, uv) in [
        (
            b"em_kyupin.ppi".as_slice(),
            [494f32, 100., 0., 48., 8., 5., 20., 5., 5.].as_slice(),
            1,
            [[0., 0.], [1., 1.]],
        ),
        (
            b"em_common.ppi".as_slice(),
            [782f32, 169., 1., 60., 5., 8.].as_slice(),
            8,
            [[0., 0.], [1., 1.]],
        ),
        (
            b"em_angry.ppi".as_slice(),
            [735f32, 192., 60., 4., 6., 70., 5., 5.].as_slice(),
            8,
            [[0., 0.], [1., 1.]],
        ),
        (
            b"em_double.ppi".as_slice(),
            [810f32, 181., 1., 60., 5., 5., 5., 5., 5.].as_slice(),
            8,
            [[0., 0.], [1., 1.]],
        ),
        (
            b"em_exclamation.ppi".as_slice(),
            [735f32, 159., 1., 60., 3., 5., 7., 60., 80.].as_slice(),
            8,
            [[0., 0.], [1., 1.]],
        ),
        (
            b"em_suprise.ppi".as_slice(),
            [804f32, 140., 1., 78., 10., 35.].as_slice(),
            8,
            [[0., 0.], [1., 1.]],
        ),
        (
            b"em_question.ppi".as_slice(),
            [719f32, 209., 1., 60., 5., 8., 10., 60., 80.].as_slice(),
            8,
            [[0., 0.], [1., 1.]],
        ),
        (
            b"em_silent.ppi".as_slice(),
            [804f32, 78., 1., 40., 13., 10., 80., 5., 5.].as_slice(),
            4,
            [[1., 0.], [0., 0.75]],
        ),
    ] {
        let mut runtime = runtime_with_count(plugin(name), count)?;
        runtime.update(1, 1)?;
        let expected = ScriptRandom::seeded(17);
        let mut parameters = [0; 128];
        for (i, value) in values.iter().copied().enumerate() {
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
        assert_eq!([first[0].uv, first[3].uv], uv);
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
        for (i, shift) in [10., 20.].into_iter().enumerate() {
            assert!((restarted[0].position[i] - first[0].position[i] - shift).abs() < 0.0001);
        }
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
    }
    Ok(())
}

#[test]
fn note_groups_particles_shares_randomness_and_preserves_task_controls() -> Result<()> {
    use katze_system2::graphics::Drawable;

    let mut runtime = runtime(plugin(b"em_note.ppi"))?;
    runtime.update(1, 1)?;
    let mut expected = ScriptRandom::seeded(17);
    let mut parameters = [0; 128];
    for (i, value) in [2f32, 778., 214., 1., 54., 30., 60., 60.]
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
    for _ in 0..72 {
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
    }
    assert_random(&mut runtime, &expected);
    let mesh = runtime
        .drawables()
        .get(1)
        .unwrap()
        .particle_mesh()
        .unwrap()
        .clone();
    let moved = mesh.projected_quad(0, &view).unwrap();
    assert_ne!(moved[0].position, first[0].position);
    assert!(moved[0].color >> 24 > 0);
    assert_eq!(mesh.projected_quad(4, &view).unwrap()[0].color, u32::MAX);
    runtime.dispatch_command(2, command(322))?;
    runtime.update(6, 1)?;
    assert_eq!(runtime.drawables().get(1).unwrap().display(), 0);
    assert_eq!(
        runtime.drawables().get(1).unwrap().particle_mesh(),
        Some(&mesh)
    );
    assert_random(&mut runtime, &expected);
    runtime.drawables_mut().set_position(100, [650., 380., 0.]);
    runtime.dispatch_command(2, command(321))?;
    for _ in 0..72 {
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
    assert_random(&mut runtime, &expected);
    assert_eq!(runtime.dispatch_command(2, command(324))?, 1);
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
fn flower_burst_shares_both_random_streams_and_preserves_task_lifecycle() -> Result<()> {
    let mut runtime = runtime(plugin(b"ohanaburst.ppi"))?;
    runtime.update(1, 1)?;
    let mut parameters = [0; 128];
    for (i, n) in [
        663f32, 248., 12., 280., 210., 40., 50., 25., 10., 30., 7., 20.,
    ]
    .into_iter()
    .enumerate()
    {
        parameters[i * 4..i * 4 + 4].copy_from_slice(&n.to_le_bytes());
    }
    runtime.dispatch_command_with_buffers(
        2,
        command(325),
        &mut CommandBuffers {
            data1: Some(&mut parameters),
            ..Default::default()
        },
    )?;
    runtime
        .drawables_mut()
        .insert(100, katze_system2::graphics::Drawable::default())?;
    runtime.drawables_mut().set_position(100, [640., 360., 0.]);
    let mut source = command(327);
    source.data[1] = 100;
    runtime.dispatch_command(2, source)?;
    runtime.dispatch_command(2, command(321))?;
    let mut expected = ScriptRandom::seeded(17);
    for _ in 0..16 {
        expected.next_integer();
    }
    for _ in 0..48 {
        expected.next_float();
    }
    assert_random(&mut runtime, &expected);
    for frame in 2..=30 {
        runtime.update(frame, 1)?;
        expected.next_integer();
    }
    assert_random(&mut runtime, &expected);
    let mesh = runtime
        .drawables()
        .get(1)
        .unwrap()
        .particle_mesh()
        .unwrap()
        .clone();
    let view = Projection::default();
    let quad = mesh.projected_quad(0, &view).unwrap();
    assert!(quad[0].color >> 24 > 0);
    assert_eq!(quad[3].uv[0] - quad[0].uv[0], 0.5);
    assert_eq!(quad[3].uv[1] - quad[0].uv[1], 0.5);
    runtime.dispatch_command(2, command(322))?;
    runtime.update(31, 1)?;
    assert_eq!(runtime.drawables().get(1).unwrap().display(), 0);
    assert_eq!(
        runtime.drawables().get(1).unwrap().particle_mesh(),
        Some(&mesh)
    );
    assert_random(&mut runtime, &expected);
    runtime.dispatch_command(2, command(321))?;
    for _ in 0..16 {
        expected.next_integer();
    }
    for _ in 0..48 {
        expected.next_float();
    }
    runtime.update(32, 1)?;
    expected.next_integer();
    assert_random(&mut runtime, &expected);
    assert_eq!(
        runtime
            .drawables()
            .get(1)
            .unwrap()
            .particle_mesh()
            .unwrap()
            .projected_quad(0, &view)
            .unwrap()[0]
            .color
            >> 24,
        0
    );
    assert_eq!(runtime.dispatch_command(2, command(324))?, 1);
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
fn film_noise_culls_hidden_layers_ignores_source_motion_and_restarts_randomness() -> Result<()> {
    use katze_system2::graphics::{Drawable, TextureFilter};
    let mut runtime = runtime(plugin(b"filmnoise.ppi"))?;
    runtime.update(1, 1)?;
    runtime.drawables_mut().insert(100, Drawable::default())?;
    let mut source = command(327);
    source.data[1] = 100;
    runtime.dispatch_command(2, source)?;
    runtime.dispatch_command(2, command(321))?;
    let mut expected = ScriptRandom::seeded(17);
    for _ in 0..30 {
        expected.next_integer();
    }
    for _ in 0..22 {
        expected.next_float();
    }
    assert_random(&mut runtime, &expected);
    runtime.update(2, 1)?;
    expected.next_integer();
    let view = Projection::default();
    let first = runtime
        .drawables()
        .get(1)
        .unwrap()
        .particle_mesh()
        .unwrap()
        .projected_quad(2, &view)
        .unwrap();
    assert_eq!([first[0].uv, first[3].uv], [[0.75, 0.5], [1., 1.]]);
    assert_ne!(first[0].color, first[1].color);
    runtime
        .drawables_mut()
        .set_position(100, [1000., 1000., 0.]);
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
    assert_eq!(mesh.projected_quad(2, &view).unwrap(), first);
    let identity = std::array::from_fn(|i| if i % 5 == 0 { 1. } else { 0. });
    assert_eq!(
        runtime
            .drawables_mut()
            .compose(&view, identity, TextureFilter::Point, None)?
            .commands
            .len(),
        2
    );
    runtime.dispatch_command(2, command(322))?;
    runtime.update(4, 1)?;
    assert_eq!(runtime.drawables().get(1).unwrap().display(), 0);
    assert_eq!(
        runtime.drawables().get(1).unwrap().particle_mesh(),
        Some(&mesh)
    );
    assert_random(&mut runtime, &expected);
    runtime.dispatch_command(2, command(321))?;
    for _ in 0..30 {
        expected.next_integer();
    }
    for _ in 0..22 {
        expected.next_float();
    }
    runtime.update(5, 1)?;
    expected.next_integer();
    assert_random(&mut runtime, &expected);
    assert_ne!(
        runtime
            .drawables()
            .get(1)
            .unwrap()
            .particle_mesh()
            .unwrap()
            .projected_quad(2, &view)
            .unwrap(),
        first
    );
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
fn scatter_effects_use_shared_randomness_and_preserve_task_lifecycle() -> Result<()> {
    for (name, values, integers, floats, uv, mesh_index, update_integers, update_floats) in [
        (
            b"asease.ppi".as_slice(),
            [782f32, 169., 62., 30., 15., 80., 120.].as_slice(),
            0,
            48,
            [[0., 0.], [1., 1.]],
            0,
            1,
            0,
        ),
        (
            b"kirakira.ppi".as_slice(),
            [607f32, 101., 21., 140., 50., 180., 10., 30.].as_slice(),
            4,
            16,
            [[0., 0.5], [0.5, 1.]],
            0,
            1,
            0,
        ),
        (
            b"warukira.ppi".as_slice(),
            [671f32, 193., 54., 230., 40., 190., 20., 15.].as_slice(),
            0,
            32,
            [[0., 0.], [1., 1.]],
            0,
            1,
            0,
        ),
        (
            b"dark.ppi".as_slice(),
            [570f32, 220., 20., 40., 50., 0.].as_slice(),
            8,
            72,
            [[0., 0.], [0.5, 0.5]],
            0,
            0,
            0,
        ),
        (
            b"sakura.ppi".as_slice(),
            [8f32, 20., 120., 500., 0.].as_slice(),
            14,
            43,
            [[0., 0.], [0.5, 0.5]],
            0,
            1,
            3,
        ),
        (
            b"hadou.ppi".as_slice(),
            [4f32, 640., 360., 1., 800., 50., 50., 20., 120., 220.].as_slice(),
            0,
            0,
            [[0., 0.], [1., 1.]],
            0,
            0,
            0,
        ),
        (
            b"concent.ppi".as_slice(),
            [4f32, 50., 15., 800., 345., 1500., 900.].as_slice(),
            4,
            4,
            [[0., 0.], [1., 1.]],
            3,
            1,
            0,
        ),
        (
            b"energy.ppi".as_slice(),
            [
                -40f32, 390., 640., 780., 90., 30., 600., 120., 40., 50., 30.,
            ]
            .as_slice(),
            8,
            40,
            [[0., 0.], [0.5, 0.5]],
            0,
            96,
            0,
        ),
        (
            b"digi.ppi".as_slice(),
            [
                640f32, 730., 640., -50., 42., 50., 50., 100., 50., 250., 5., 1000., 200., 1.,
            ]
            .as_slice(),
            48,
            72,
            [[0., 0.], [0.5, 0.5]],
            0,
            0,
            0,
        ),
        (
            b"digiring.ppi".as_slice(),
            [
                640f32, 730., 640., -180., 47., 10., 10., 50., 50., 250., 30., 350., 1000., 200.,
                1.,
            ]
            .as_slice(),
            32,
            64,
            [[0., 0.], [0.5, 0.5]],
            0,
            0,
            0,
        ),
        (
            b"rising4.ppi".as_slice(),
            [580f32, 500., 0., 40., 0., 0., 580., -40., 0.].as_slice(),
            0,
            1,
            [[0., 0.5], [0.5, 1.]],
            0,
            0,
            4,
        ),
        (
            b"rising3.ppi".as_slice(),
            [640f32, 360., 0., 80., 1., 15., 640., -80., 0.].as_slice(),
            0,
            1,
            [[0., 0.5], [0.5, 1.]],
            0,
            0,
            4,
        ),
        (
            b"em_shy.ppi".as_slice(),
            [
                10f32, 30., 0., 90., 15., 10., 20., 10., 15., 100., 100., 612., 165., 652., 165.,
                60., 60., 0.,
            ]
            .as_slice(),
            32,
            78,
            [[0., 0.], [0.5, 0.5]],
            0,
            1,
            0,
        ),
    ] {
        let mut runtime = runtime(plugin(name))?;
        runtime.update(1, 1)?;
        let mut parameters = [0; 128];
        for (i, n) in values.iter().copied().enumerate() {
            parameters[i * 4..i * 4 + 4].copy_from_slice(&n.to_le_bytes());
        }
        runtime.dispatch_command_with_buffers(
            2,
            command(325),
            &mut CommandBuffers {
                data1: Some(&mut parameters),
                ..Default::default()
            },
        )?;
        runtime
            .drawables_mut()
            .insert(100, katze_system2::graphics::Drawable::default())?;
        runtime.drawables_mut().set_position(100, [640., 360., 0.]);
        let mut source = command(327);
        source.data[1] = 100;
        runtime.dispatch_command(2, source)?;
        runtime.dispatch_command(2, command(321))?;
        let mut expected = ScriptRandom::seeded(17);
        for _ in 0..integers {
            expected.next_integer();
        }
        for _ in 0..floats {
            expected.next_float();
        }
        assert_random(&mut runtime, &expected);
        for frame in 2..=10 {
            runtime.update(frame, 1)?;
            for _ in 0..update_integers {
                expected.next_integer();
            }
            for _ in 0..update_floats {
                expected.next_float();
            }
        }
        assert_random(&mut runtime, &expected);
        let mesh = runtime
            .drawables()
            .get(1)
            .unwrap()
            .particle_mesh()
            .unwrap()
            .clone();
        let quad = mesh
            .projected_quad(mesh_index, &Projection::default())
            .unwrap();
        assert!(quad[0].color >> 24 > 0);
        assert_eq!([quad[0].uv, quad[3].uv], uv);
        runtime.dispatch_command(2, command(322))?;
        runtime.update(11, 1)?;
        assert_eq!(runtime.drawables().get(1).unwrap().display(), 0);
        assert_eq!(
            runtime.drawables().get(1).unwrap().particle_mesh(),
            Some(&mesh)
        );
        assert_random(&mut runtime, &expected);
        if matches!(name, b"concent.ppi" | b"hadou.ppi") {
            let value = parameters[20..24].to_vec();
            parameters[20..24].fill(0);
            runtime.dispatch_command_with_buffers(
                2,
                command(325),
                &mut CommandBuffers {
                    data1: Some(&mut parameters),
                    ..Default::default()
                },
            )?;
            assert!(runtime.dispatch_command(2, command(321)).is_err());
            assert_eq!(runtime.dispatch_command(2, command(324))?, 0);
            assert_eq!(runtime.drawables().get(1).unwrap().display(), 0);
            assert_random(&mut runtime, &expected);
            parameters[20..24].copy_from_slice(&value);
            runtime.dispatch_command_with_buffers(
                2,
                command(325),
                &mut CommandBuffers {
                    data1: Some(&mut parameters),
                    ..Default::default()
                },
            )?;
        }
        runtime.dispatch_command(2, command(321))?;
        for _ in 0..integers {
            expected.next_integer();
        }
        // 曲线分支使用共享整数序列，重启时的浮点消费量随其当前位置变化。
        let restart_floats = if name == b"em_shy.ppi" { 80 } else { floats };
        for _ in 0..restart_floats {
            expected.next_float();
        }
        runtime.update(12, 1)?;
        for _ in 0..update_integers {
            expected.next_integer();
        }
        for _ in 0..update_floats {
            expected.next_float();
        }
        assert_random(&mut runtime, &expected);
        let restarted = runtime
            .drawables()
            .get(1)
            .unwrap()
            .particle_mesh()
            .unwrap()
            .projected_quad(mesh_index, &Projection::default())
            .unwrap();
        if matches!(
            name,
            b"digi.ppi" | b"digiring.ppi" | b"rising4.ppi" | b"rising3.ppi" | b"sakura.ppi"
        ) {
            assert!(restarted[0].color >> 24 > 0);
        } else {
            assert_eq!(restarted[0].color >> 24, 0);
        }
        assert_ne!(restarted[0].position, quad[0].position);
        assert_eq!(runtime.dispatch_command(2, command(324))?, 1);
        runtime.dispatch_command(2, command(323))?;
        assert!(
            runtime
                .drawables()
                .get(1)
                .unwrap()
                .particle_mesh()
                .is_none()
        );
    }
    Ok(())
}

#[test]
fn protect_initializes_without_a_flash_and_preserves_blending_when_restarted() -> Result<()> {
    let mut runtime = runtime_with_count(plugin(b"protect.ppi"), 26)?;
    runtime.update(1, 1)?;
    let mut parameters = [0; 128];
    for (i, value) in [390f32, 360., 170., -400., 400., 0.]
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
    let expected = ScriptRandom::seeded(17);
    for start in [2, 202] {
        runtime.dispatch_command(2, command(321))?;
        let drawable = runtime.drawables().get(1).unwrap();
        assert_eq!(drawable.blend_mode(), u32::from(start != 2));
        let mesh = drawable.particle_mesh().unwrap();
        let quad = mesh.projected_quad(1, &Projection::default()).unwrap();
        assert!(quad.iter().all(|v| v.position == quad[0].position));
        for frame in start..start + 200 {
            runtime.update(frame, 1)?;
        }
        let drawable = runtime.drawables().get(1).unwrap();
        assert_eq!(drawable.blend_mode(), 1);
        let quad = drawable
            .particle_mesh()
            .unwrap()
            .projected_quad(13, &Projection::default())
            .unwrap();
        assert!(quad.iter().all(|v| v.color >> 24 == 0));
        assert_eq!(runtime.dispatch_command(2, command(324))?, 1);
        assert_random(&mut runtime, &expected);
        runtime.dispatch_command(2, command(322))?;
        assert_eq!(runtime.drawables().get(1).unwrap().display(), 0);
    }
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
fn ice_uses_additive_transparent_start_shared_randomness_and_task_controls() -> Result<()> {
    let mut runtime = runtime_with_count(plugin(b"o_150102.ppi"), 21)?;
    runtime.update(1, 1)?;
    let mut parameters = [0; 128];
    for (i, n) in [210f32, 560., 900., -800., 15., 10.]
        .into_iter()
        .enumerate()
    {
        parameters[i * 4..i * 4 + 4].copy_from_slice(&n.to_le_bytes());
    }
    runtime.dispatch_command_with_buffers(
        2,
        command(325),
        &mut CommandBuffers {
            data1: Some(&mut parameters),
            ..Default::default()
        },
    )?;
    let mut expected = ScriptRandom::seeded(17);
    for frame in [2, 20] {
        runtime.dispatch_command(2, command(321))?;
        for _ in 0..14 {
            expected.next_integer();
            expected.next_float();
        }
        assert_random(&mut runtime, &expected);
        let drawable = runtime.drawables().get(1).unwrap();
        assert_eq!(drawable.blend_mode(), 1);
        let mesh = drawable.particle_mesh().unwrap();
        assert!((0..21).all(|i| {
            mesh.projected_quad(i, &Projection::default())
                .unwrap()
                .iter()
                .all(|v| v.color >> 24 == 0)
        }));
        runtime.update(frame, 1)?;
        for _ in 0..14 {
            expected.next_integer();
        }
        for _ in 0..140 {
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
        assert!(
            (7..21).any(
                |i| mesh.projected_quad(i, &Projection::default()).unwrap()[0].color >> 24 > 0
            )
        );
        runtime.dispatch_command(2, command(322))?;
        runtime.update(frame + 1, 1)?;
        let drawable = runtime.drawables().get(1).unwrap();
        assert_eq!(drawable.display(), 0);
        assert_eq!(drawable.particle_mesh(), Some(&mesh));
        assert_random(&mut runtime, &expected);
    }
    assert_eq!(runtime.dispatch_command(2, command(324))?, 0);
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
fn scene_sparkle_initializes_transparently_and_restarts_with_shared_randomness() -> Result<()> {
    let mut runtime = runtime_with_count(plugin(b"s_040101.ppi"), 1)?;
    runtime.update(1, 1)?;
    let mut parameters = [0; 128];
    for (i, n) in [
        9f32, 12.5, -8., -35., 220., 137., 7.8, 782., 169., 44., -120., -60., -3., 137., 15.25,
        7.8, 0., 3.5, 45., 1.,
    ]
    .into_iter()
    .enumerate()
    {
        parameters[i * 4..i * 4 + 4].copy_from_slice(&n.to_le_bytes());
    }
    runtime.dispatch_command_with_buffers(
        2,
        command(325),
        &mut CommandBuffers {
            data1: Some(&mut parameters),
            ..Default::default()
        },
    )?;
    let mut expected = ScriptRandom::seeded(17);
    let view = Projection::default();
    let mut positions = Vec::new();
    for start in [2, 20] {
        runtime.dispatch_command(2, command(321))?;
        for _ in 0..5 {
            expected.next_integer();
        }
        for _ in 0..21 {
            expected.next_float();
        }
        assert_random(&mut runtime, &expected);
        assert_eq!(runtime.dispatch_command(2, command(324))?, 1);
        let drawable = runtime.drawables().get(1).unwrap();
        assert_eq!(drawable.blend_mode(), 1);
        let quad = drawable
            .particle_mesh()
            .unwrap()
            .projected_quad(0, &view)
            .unwrap();
        assert!(quad.iter().all(|v| v.color >> 24 == 0));
        positions.push(quad[0].position);
        for frame in start..=start + 4 {
            runtime.update(frame, 1)?;
        }
        let mesh = runtime
            .drawables()
            .get(1)
            .unwrap()
            .particle_mesh()
            .unwrap()
            .clone();
        let quad = mesh.projected_quad(0, &view).unwrap();
        assert_eq!(quad[0].color >> 24, 73);
        assert_eq!(
            [quad[0].uv, quad[3].uv],
            if start == 2 {
                [[0., 0.], [0.5, 0.5]]
            } else {
                [[0., 0.5], [0.5, 1.]]
            }
        );
        runtime.dispatch_command(2, command(322))?;
        runtime.update(start + 5, 1)?;
        assert_eq!(runtime.drawables().get(1).unwrap().display(), 0);
        assert_eq!(
            runtime.drawables().get(1).unwrap().particle_mesh(),
            Some(&mesh)
        );
        assert_random(&mut runtime, &expected);
    }
    assert_ne!(positions[0], positions[1]);
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
fn spark_path_variant_registers_restarts_stops_and_rejects_the_other_layout() -> Result<()> {
    let mut runtime = runtime_with_count(plugin(b"spark.ppi"), 3)?;
    runtime.update(1, 1)?;
    let mut parameters = [0; 128];
    for (i, n) in [
        125f32, -70., 530., 410., 65., 15., 3., 8., 5., 20., 70., 2., 80., 60., 1., 0., 1.,
    ]
    .into_iter()
    .enumerate()
    {
        parameters[i * 4..i * 4 + 4].copy_from_slice(&n.to_le_bytes());
    }
    runtime.dispatch_command_with_buffers(
        2,
        command(325),
        &mut CommandBuffers {
            data1: Some(&mut parameters),
            ..Default::default()
        },
    )?;
    let view = Projection::default();
    let mut starts = Vec::new();
    for start in [2, 20] {
        runtime.dispatch_command(2, command(321))?;
        assert_eq!(runtime.dispatch_command(2, command(324))?, 1);
        let drawable = runtime.drawables().get(1).unwrap();
        assert_eq!(drawable.blend_mode(), 1);
        let quad = drawable
            .particle_mesh()
            .unwrap()
            .projected_quad(0, &view)
            .unwrap();
        assert!(quad.iter().all(|v| v.color >> 24 == 0));
        starts.push(quad[0].position);
        for now in start..=start + 5 {
            runtime.update(now, 1)?;
        }
        assert!(
            runtime
                .drawables()
                .get(1)
                .unwrap()
                .particle_mesh()
                .unwrap()
                .projected_quad(0, &view)
                .unwrap()[0]
                .color
                >> 24
                != 0
        );
        runtime.dispatch_command(2, command(322))?;
        let random = runtime.host_mut().random.clone();
        runtime.update(start + 6, 1)?;
        assert_random(&mut runtime, &random);
        assert_eq!(runtime.drawables().get(1).unwrap().display(), 0);
    }
    assert_ne!(starts[0], starts[1]);
    runtime.destroy(1)?;
    assert!(runtime.drawables().get(1).is_none());

    let mut other = plugin(b"spark.ppi");
    other[400..403].copy_from_slice(&[0x6a, 0x40, 0xe8]);
    assert!(runtime_with_count(other, 3)?.update(1, 1).is_err());
    Ok(())
}

#[test]
fn thunder_finishes_hides_and_preserves_module_randomness_across_restarts() -> Result<()> {
    let mut runtime = runtime_with_count(plugin(b"thunder.ppi"), 3)?;
    runtime.update(1, 1)?;
    let mut parameters = [0; 128];
    for (i, n) in [3f32, 640., 360., 100., 4., 3., 5.].into_iter().enumerate() {
        parameters[i * 4..i * 4 + 4].copy_from_slice(&n.to_le_bytes());
    }
    runtime.dispatch_command_with_buffers(
        2,
        command(325),
        &mut CommandBuffers {
            data1: Some(&mut parameters),
            ..Default::default()
        },
    )?;
    let view = Projection::default();
    let mut expected = ScriptRandom::seeded(17);
    for (start, region) in [(2, 0.25), (30, 0.75)] {
        runtime.dispatch_command(2, command(321))?;
        expected.next_float();
        assert_random(&mut runtime, &expected);
        let drawable = runtime.drawables().get(1).unwrap();
        assert_eq!(drawable.blend_mode(), 1);
        let quad = drawable
            .particle_mesh()
            .unwrap()
            .projected_quad(0, &view)
            .unwrap();
        assert_eq!(quad[0].uv, [region, 0.]);
        assert_eq!(quad[0].color >> 24, 0);
        let mut visible = false;
        for now in start..start + 24 {
            runtime.update(now, 1)?;
            let drawable = runtime.drawables().get(1).unwrap();
            visible |= drawable
                .particle_mesh()
                .unwrap()
                .projected_quad(0, &view)
                .unwrap()[0]
                .color
                >> 24
                != 0;
        }
        assert!(visible);
        assert_eq!(runtime.dispatch_command(2, command(324))?, 0);
        assert_eq!(runtime.drawables().get(1).unwrap().display(), 0);
        assert_random(&mut runtime, &expected);
    }
    runtime.dispatch_command(2, command(321))?;
    expected.next_float();
    runtime.dispatch_command(2, command(322))?;
    runtime.update(60, 1)?;
    assert_random(&mut runtime, &expected);
    assert_eq!(runtime.drawables().get(1).unwrap().display(), 0);
    runtime.destroy(1)?;
    assert!(runtime.drawables().get(1).is_none());
    Ok(())
}
