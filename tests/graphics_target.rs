use katze_system2::{
    Result,
    graphics::{DrawingContext, Rect, RenderTarget, Surface},
    kcs::Program,
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
};

fn tile(values: &[u32]) -> Surface {
    Surface::from_bgra(
        [3, 2],
        values.iter().flat_map(|v| v.to_le_bytes()).collect(),
    )
    .unwrap()
}

#[test]
fn target_clears_top_origin_regions_and_marks_only_intersecting_present_tiles() {
    let mut target = RenderTarget::from_tiles(
        [3, 2],
        [2, 2],
        vec![
            Some(tile(&[1, 2, 3, 4, 5, 0x8044_5566])),
            None,
            Some(tile(&[13, 14, 0xffaa_bbcc, 16, 17, 18])),
            Some(tile(&[0xfe11_2233, 20, 21, 22, 23, 24])),
        ],
    )
    .unwrap();
    assert_eq!(target.size(), [6, 4]);
    assert_eq!(target.tile_size(), [3, 2]);
    assert_eq!(target.grid(), [2, 2]);
    assert!(target.clear_rect(Rect::from_xywh(2, 1, 2, 2)));
    assert_eq!(target.dirty_tiles(), [true, false, true, true]);
    let expected = [
        [Some(1), Some(2), Some(3), None, None, None],
        [Some(4), Some(5), Some(0), None, None, None],
        [Some(13), Some(14), Some(0), Some(0), Some(20), Some(21)],
        [Some(16), Some(17), Some(18), Some(22), Some(23), Some(24)],
    ];
    for (y, row) in expected.iter().enumerate() {
        for (x, &value) in row.iter().enumerate() {
            assert_eq!(target.pixel(x as u32, y as u32), value);
        }
    }
    assert_eq!(target.pixel(6, 0), None);
    target.clear_dirty();
    assert_eq!(target.dirty_tiles(), [false; 4]);
    assert!(target.clear_rect(Rect::from_xywh(2, 1, 1, 1)));
    assert_eq!(target.dirty_tiles(), [true, false, false, false]);
    target.clear_dirty();
    for rect in [
        Rect::from_xywh(3, 0, 3, 2),
        Rect::from_xywh(6, 0, 1, 1),
        Rect::from_xywh(0, 0, -1, 1),
        Rect::from_xywh(i32::MAX, 0, 4, 1),
    ] {
        assert!(!target.clear_rect(rect));
    }
    assert_eq!(target.dirty_tiles(), [false; 4]);
    assert!(target.clear_rect(Rect::from_xywh(-1, -1, 3, 2)));
    assert_eq!(target.pixel(0, 0), Some(0));
    assert_eq!(target.pixel(1, 0), Some(0));
    assert_eq!(target.pixel(2, 0), Some(3));
    assert_eq!(target.pixel(0, 1), Some(4));
}

#[test]
fn drawing_gates_preserve_pixels_and_update_markers_until_all_conditions_hold() {
    let target = RenderTarget::from_tiles([3, 2], [1, 1], vec![Some(tile(&[7; 6]))]).unwrap();
    for (mode, enabled, active, present, clears) in [
        (0, true, true, true, true),
        (1, true, true, true, false),
        (u32::MAX, true, true, true, false),
        (0, false, true, true, false),
        (0, true, false, true, false),
        (0, true, true, false, false),
    ] {
        let mut drawing = DrawingContext {
            mode,
            enabled,
            active,
            target: present.then(|| target.clone()),
        };
        assert_eq!(drawing.clear_rect(Rect::from_xywh(1, 0, 1, 1)), clears);
        if let Some(target) = &drawing.target {
            assert_eq!(target.pixel(1, 0), Some(if clears { 0 } else { 7 }));
            assert_eq!(target.pixel(1, 1), Some(7));
            assert_eq!(target.dirty_tiles(), [clears]);
        }
    }
    assert!(!DrawingContext::default().clear_rect(Rect::from_xywh(0, 0, 1, 1)));
}

#[test]
fn target_layouts_reject_mismatches_overflow_and_excessive_storage() {
    assert!(RenderTarget::from_tiles([3, 2], [2, 1], vec![None]).is_err());
    assert!(RenderTarget::from_tiles([2, 2], [1, 1], vec![Some(tile(&[0; 6]))]).is_err());
    assert!(RenderTarget::from_tiles([u32::MAX, 1], [2, 0], vec![]).is_err());
    assert!(RenderTarget::from_tiles([0, 0], [1_000_001, 1], vec![]).is_err());
    assert!(RenderTarget::new([4097, 4096]).is_err());
    for size in [[0, 0], [0, 3], [3, 0]] {
        let mut target = RenderTarget::new(size).unwrap();
        assert!(!target.clear_rect(Rect::from_xywh(-1, -1, 5, 5)));
        assert_eq!(target.pixel(0, 0), None);
        assert_eq!(target.dirty_tiles(), [false]);
    }
}

struct Programs;
impl ProgramLoader for Programs {
    fn load(&mut self, name: &[u8]) -> Result<Option<Program>> {
        fn ins(code: &mut Vec<u8>, op: u8, args: &[u32]) {
            code.push(op);
            for arg in args {
                code.push(3);
                code.extend(arg.to_le_bytes());
            }
        }
        let short = name == b"short";
        let mut code = Vec::new();
        ins(&mut code, 0x25, &[0xdead_beef]);
        for n in [0, u32::from(!short), u32::from(short), 1, 1] {
            ins(&mut code, 0x25, &[n]);
        }
        if !short {
            ins(&mut code, 0x25, &[0xff12_3456]);
        }
        ins(&mut code, 0x2b, &[117, if short { 20 } else { 24 }]);
        ins(&mut code, 0x26, &[0]);
        for address in [4, 8] {
            ins(&mut code, 0x25, &[address]);
        }
        ins(&mut code, 0x2b, &[8, 8]);
        ins(&mut code, 0x3b, &[]);
        Ok(Some(Program::from_code(&code, vec![0; 16], 128)?))
    }
}

#[test]
fn runtime_clear_uses_the_shared_target_and_preserves_stack_and_configured_size() {
    for budget in [1, 4096] {
        for present in [false, true] {
            let mut runtime = Runtime::new(
                0,
                kind::ROOT,
                Box::new(BaseObject::new()),
                katze_system2::expression::State::new(0),
                KcsFactory::new(Programs),
            );
            runtime.set_budget(budget, 1 << 20);
            runtime.host_mut().surface_size = [1024, 768];
            *runtime.drawing_mut() = DrawingContext {
                mode: 0,
                enabled: true,
                active: true,
                target: present.then(|| {
                    RenderTarget::from_tiles([3, 2], [1, 1], vec![Some(tile(&[7; 6]))]).unwrap()
                }),
            };
            let ids = [b"full".as_slice(), b"short"]
                .map(|name| runtime.create(0, kind::ORDINARY, name, b"").unwrap());
            runtime.update(0, 0).unwrap();
            for id in ids {
                let vm = runtime.object(id).unwrap().interpreter().unwrap();
                assert_eq!(
                    vm.read(0, 12).unwrap(),
                    [0xdead_beefu32, 1024, 768]
                        .into_iter()
                        .flat_map(u32::to_le_bytes)
                        .collect::<Vec<_>>()
                );
            }
            if let Some(target) = &runtime.drawing().target {
                assert_eq!(target.pixel(1, 0), Some(0));
                assert_eq!(target.pixel(0, 1), Some(0));
                assert_eq!(target.pixel(1, 1), Some(7));
                assert_eq!(target.dirty_tiles(), [true]);
            }
        }
    }
}
