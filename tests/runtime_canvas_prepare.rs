use katze_system2::{
    Result,
    expression::State,
    graphics::{DrawingContext, RenderTarget},
    host::graphics::{Rect, Surface},
    kcs::Program,
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
    settings::SystemSettings,
};

struct Loader(Program);

impl ProgramLoader for Loader {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        Ok(Some(self.0.clone()))
    }
}

fn instruction(code: &mut Vec<u8>, opcode: u8, arguments: &[u32]) {
    code.push(opcode);
    for argument in arguments {
        code.push(3);
        code.extend(argument.to_le_bytes());
    }
}

#[test]
fn preparation_keeps_existing_tiles_and_pixels_across_yields_and_ignores_active() {
    let mut code = vec![];
    instruction(&mut code, 0x25, &[0x1234_5678]);
    instruction(&mut code, 0x25, &[u32::MAX]);
    instruction(&mut code, 0x2b, &[542, 4]);
    instruction(&mut code, 0x2b, &[65, 0]);
    instruction(&mut code, 0x2b, &[542, 0]);
    code.push(0x3b);
    let program = Program::from_code(&code, vec![], 64).unwrap();
    for budget in [1, 4096] {
        for (mode, enabled, active, present) in [
            (0, true, false, true),
            (0, true, true, true),
            (1, true, true, true),
            (0, false, true, true),
            (0, true, true, false),
        ] {
            let mut runtime = Runtime::new(
                0,
                kind::ROOT,
                Box::new(BaseObject::new()),
                State::new(0),
                KcsFactory::new(Loader(program.clone())),
            );
            runtime.set_budget(budget, 1 << 20);
            let mut existing = Surface::new([2, 1]).unwrap();
            existing.clear(0x8123_4567);
            *runtime.drawing_mut() = DrawingContext {
                mode,
                enabled,
                active,
                target: present.then(|| {
                    RenderTarget::from_tiles([2, 1], [2, 2], vec![Some(existing), None, None, None])
                        .unwrap()
                }),
            };
            let eligible = mode == 0 && enabled && present;
            if eligible {
                let mut settings = SystemSettings {
                    texture_16bit_row: 1,
                    ..SystemSettings::default()
                };
                settings.texture_areas[0] = [0, 0, 2, 1];
                settings.texture_areas[1] = [0, 1, 1, 1];
                runtime.host_mut().system_settings = Some(settings);
            }
            let task = runtime.create(0, kind::ORDINARY, b"canvas", b"").unwrap();
            runtime.update(0, 1).unwrap();
            if let Some(target) = &runtime.drawing_mut().target {
                assert_eq!(target.pixel(0, 0), Some(0x8123_4567));
                assert_eq!(target.tile_bit_depth(0), Some(32));
                assert_eq!(target.tile_bit_depth(1), eligible.then_some(32));
                assert_eq!(target.tile_bit_depth(2), eligible.then_some(16));
                assert_eq!(target.tile_bit_depth(3), None);
                assert_eq!(target.dirty_tiles(), [false; 4]);
            }
            if eligible {
                let mut source = Surface::new([4, 2]).unwrap();
                source.clear(0x8123_4567);
                let target = runtime.drawing_mut().target.as_mut().unwrap();
                target.copy_surface(&source, Rect::from_xywh(0, 0, 4, 2), [0, 0]);
                assert_eq!(target.pixel(0, 0), Some(0x8123_4567));
                assert_eq!(target.pixel(0, 1), Some(0x8822_4466));
                runtime
                    .host_mut()
                    .system_settings
                    .as_mut()
                    .unwrap()
                    .texture_areas[1][2] = 2;
            }
            runtime.update(1, 1).unwrap();
            if eligible {
                let target = runtime.drawing_mut().target.as_ref().unwrap();
                assert_eq!(target.pixel(0, 0), Some(0x8123_4567));
                assert_eq!(target.pixel(0, 1), Some(0x8822_4466));
                assert_eq!(target.pixel(2, 1), Some(0));
                assert_eq!(target.tile_bit_depth(3), Some(16));
                assert_eq!(target.dirty_tiles(), [true, true, true, false]);
            }
            assert_eq!(
                runtime
                    .object(task)
                    .unwrap()
                    .interpreter()
                    .unwrap()
                    .stack_bytes(),
                0x1234_5678u32.to_le_bytes()
            );
        }
    }
    let sparse = RenderTarget::from_tiles([256, 256], [12, 31], vec![None; 372]).unwrap();
    assert_eq!(sparse.size(), [3072, 7936]);
}

#[test]
fn release_drops_intersecting_tiles_and_preparation_recreates_zeroed_surfaces() {
    for budget in [1, 4096] {
        for (mode, enabled, active, present) in [
            (0, true, false, true),
            (0, true, true, true),
            (1, true, true, true),
            (0, false, true, true),
            (0, true, true, false),
        ] {
            let eligible = mode == 0 && enabled && present;
            let mut code = vec![];
            instruction(&mut code, 0x25, &[0x12345678]);
            instruction(&mut code, 0x25, &[0x80000001]);
            instruction(&mut code, 0x2b, &[577, 4]);
            for value in if eligible { &[1, 0, 2, 3][..] } else { &[] } {
                instruction(&mut code, 0x25, &[*value]);
            }
            instruction(&mut code, 0x2b, &[541, if eligible { 16 } else { 0 }]);
            instruction(&mut code, 0x2b, &[65, 0]);
            instruction(&mut code, 0x2b, &[542, 0]);
            code.push(0x3b);
            let program = Program::from_code(&code, vec![], 64).unwrap();
            let mut runtime = Runtime::new(
                0,
                kind::ROOT,
                Box::new(katze_system2::runtime::RootObject::new(0, 0)),
                State::new(0),
                KcsFactory::new(Loader(program)),
            );
            runtime.set_budget(budget, 1 << 20);
            let mut target = RenderTarget::from_tiles(
                [2, 2],
                [3, 2],
                (0..6)
                    .map(|index| {
                        if index == 4 {
                            return None;
                        }
                        let mut surface = Surface::new([2, 2]).unwrap();
                        surface.clear(0xff000000 | index);
                        Some(surface)
                    })
                    .collect(),
            )
            .unwrap();
            target.clear_rect(Rect::from_xywh(0, 0, 1, 4));
            *runtime.drawing_mut() = DrawingContext {
                mode,
                enabled,
                active,
                target: present.then_some(target),
            };
            if eligible {
                let mut settings = SystemSettings {
                    texture_16bit_row: 1,
                    ..SystemSettings::default()
                };
                settings.texture_areas[0] = [0, 0, 2, 2];
                runtime.host_mut().system_settings = Some(settings);
            }
            let before = runtime.drawing_mut().target.as_mut().map(|target| {
                let canvas = target.frame_canvas();
                std::array::from_fn::<_, 6, _>(|index| canvas.tile_version(index))
            });
            let task = runtime.create(0, kind::ORDINARY, b"release", b"").unwrap();
            runtime.update(0, 1).unwrap();
            if let Some(target) = runtime.drawing().target.as_ref() {
                assert_eq!(
                    target
                        .tiles()
                        .iter()
                        .map(Option::is_some)
                        .collect::<Vec<_>>(),
                    if eligible {
                        vec![false, false, true, false, false, true]
                    } else {
                        vec![true, true, true, true, false, true]
                    }
                );
                assert_eq!(target.pixel(4, 0), Some(0xff000002));
                assert_eq!(target.pixel(4, 3), Some(0xff000005));
                assert_eq!(
                    target.dirty_tiles(),
                    [true, false, false, true, false, false]
                );
            }
            runtime.update(1, 1).unwrap();
            if eligible {
                let target = runtime.drawing().target.as_ref().unwrap();
                assert!(target.tiles().iter().all(Option::is_some));
                assert_eq!(target.pixel(1, 1), Some(0));
                assert_eq!(target.pixel(3, 3), Some(0));
                assert_eq!(target.pixel(4, 0), Some(0xff000002));
                assert_eq!(target.tile_bit_depth(3), Some(16));
                assert_eq!(target.tile_bit_depth(5), Some(32));
                assert_eq!(
                    target.dirty_tiles(),
                    [true, false, false, true, false, false]
                );
                let before = before.unwrap();
                let mut canvas = runtime
                    .drawing_mut()
                    .target
                    .as_mut()
                    .unwrap()
                    .frame_canvas();
                assert_ne!(canvas.tile_version(1), before[1]);
                assert!(canvas.tile_version(4).is_some());
                assert_eq!(canvas.tile_version(2), before[2]);
                assert!(!canvas.dirty_tiles()[1]);
                let recreated = canvas.tile_version(1);
                canvas.acknowledge_upload();
                assert_eq!(canvas.tile_version(1), recreated);
            }
            assert_eq!(
                runtime
                    .object(task)
                    .unwrap()
                    .interpreter()
                    .unwrap()
                    .stack_bytes(),
                0x12345678u32.to_le_bytes()
            );
        }
    }
}
