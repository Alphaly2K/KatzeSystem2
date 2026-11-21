use katze_system2::{
    Result,
    expression::State,
    input::CoordinateMap,
    kcs::Program,
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
};

struct Loader(Program);
impl ProgramLoader for Loader {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        Ok(Some(self.0.clone()))
    }
}

#[test]
fn render_offset_query_keeps_shared_outputs_gate_order_and_void_return() {
    fn ins(code: &mut Vec<u8>, op: u8, args: &[u32]) {
        code.push(op);
        for value in args {
            code.push(3);
            code.extend(value.to_le_bytes());
        }
    }
    for budget in [1, 4096] {
        for (mode, enabled, addresses, expected) in [
            (0, true, [0, 4], [9i32, 0]),
            (0, true, [0, 0], [7, 7]),
            (1, true, [0, 4], [-18, -1]),
            (0, false, [0, 0], [2, 2]),
            (0, true, [0x80000000, 0], [9, 0]),
        ] {
            let mut code = vec![];
            for address in addresses {
                ins(&mut code, 0x25, &[address]);
            }
            ins(&mut code, 0x2b, &[739, 8]);
            ins(&mut code, 0x2b, &[65, 0]);
            let program = Program::from_code(&code, vec![0xaa; 16], 32).unwrap();
            let mut factory = KcsFactory::new(Loader(program));
            factory.external = katze_system2::memory::SharedMemory::new(vec![0; 4]).unwrap();
            let mut runtime = Runtime::new(
                0,
                kind::BASE,
                Box::new(BaseObject::new()),
                State::new(0),
                factory,
            );
            runtime.set_budget(budget, 10000);
            runtime.drawing_mut().mode = mode;
            runtime.drawing_mut().enabled = enabled;
            *runtime.input_coordinates_mut() = CoordinateMap {
                device_offset: [10.75, -3.25],
                device_scale: 2.,
                configuration_offset: [1, -2],
                logical_scale: [3., -0.5],
                ..CoordinateMap::default()
            };
            let task = runtime.create(0, kind::ORDINARY, b"offset", b"").unwrap();
            runtime.update(0, 1).unwrap();
            let vm = runtime.object(task).unwrap().interpreter().unwrap();
            for (address, value) in addresses.into_iter().zip(expected) {
                assert_eq!(vm.read(address, 4).unwrap(), value.to_le_bytes());
            }
            assert_eq!(vm.read(8, 8).unwrap(), [0xaa; 8]);
            assert!(vm.stack_bytes().is_empty());
            assert!(!runtime.drawing().active);
            assert!(runtime.drawing().target.is_none());
        }
    }
}

#[test]
fn pointer_exit_query_keeps_position_modes_mapping_and_aliased_outputs() {
    fn ins(code: &mut Vec<u8>, op: u8, args: &[u32]) {
        code.push(op);
        for value in args {
            code.push(3);
            code.extend(value.to_le_bytes());
        }
    }
    for budget in [1, 4096] {
        for (position, mode, enabled, addresses, expected, external) in [
            (
                0,
                0,
                true,
                [0, 4],
                [0xffff_ffee, 0xffff_ff67],
                [0xbbbb_bbbb, 0xbbbb_bbbb],
            ),
            (
                1,
                0,
                true,
                [0, 4],
                [0xffff_ffee, 0xffff_fef1],
                [0xbbbb_bbbb, 0xbbbb_bbbb],
            ),
            (
                2,
                0,
                true,
                [0, 4],
                [0x0000_0000, 0x0000_02d0],
                [0xbbbb_bbbb, 0xbbbb_bbbb],
            ),
            (
                3,
                0,
                true,
                [0, 4],
                [0x0000_0000, 0xaaaa_aaaa],
                [0xbbbb_bbbb, 0xbbbb_bbbb],
            ),
            (
                0,
                0,
                false,
                [0, 4],
                [0xffff_ffee, 0xffff_ff1e],
                [0xbbbb_bbbb, 0xbbbb_bbbb],
            ),
            (
                0,
                1,
                true,
                [0, 4],
                [0xffff_ffee, 0xffff_ff1e],
                [0xbbbb_bbbb, 0xbbbb_bbbb],
            ),
            (
                0,
                0,
                true,
                [0, 0],
                [0xffff_ff21, 0xaaaa_aaaa],
                [0xbbbb_bbbb, 0xbbbb_bbbb],
            ),
            (
                0,
                0,
                true,
                [0, 1],
                [0xffff_1ceb, 0xaaaa_aaff],
                [0xbbbb_bbbb, 0xbbbb_bbbb],
            ),
            (
                0,
                0,
                true,
                [1, 0],
                [0x0000_03a6, 0xaaaa_aaff],
                [0xbbbb_bbbb, 0xbbbb_bbbb],
            ),
            (
                0,
                0,
                true,
                [2147483648, 4],
                [0xaaaa_aaaa, 0xffff_ff67],
                [0xffff_ffee, 0xbbbb_bbbb],
            ),
            (
                0,
                0,
                true,
                [0, 2147483648],
                [0xffff_ffee, 0xaaaa_aaaa],
                [0xffff_ff67, 0xbbbb_bbbb],
            ),
            (
                0,
                0,
                true,
                [2147483651, 2147483648],
                [0xaaaa_aaaa, 0xaaaa_aaaa],
                [0x047f_ff68, 0xbbff_ffff],
            ),
        ] {
            let mut code = vec![];
            for address in addresses {
                ins(&mut code, 0x25, &[address]);
            }
            ins(&mut code, 0x2b, &[740, 8]);
            ins(&mut code, 0x2b, &[65, 0]);
            let program = Program::from_code(&code, vec![0xaa; 32], 32).unwrap();
            let mut factory = KcsFactory::new(Loader(program));
            factory.external = katze_system2::memory::SharedMemory::new(vec![0xbb; 32]).unwrap();
            let mut runtime = Runtime::new(
                0,
                kind::BASE,
                Box::new(BaseObject::new()),
                State::new(0),
                factory,
            );
            runtime.set_budget(budget, 10000);
            runtime.host_mut().platform_settings =
                Some(katze_system2::settings::PlatformSettings {
                    window_size: [1280, 720],
                    fullscreen_size: [1920, 1080],
                    full_screen_full_pos: position,
                    ..Default::default()
                });
            runtime.set_surface_size([1600, 900]);
            runtime.set_window_content_rect(if mode == 0 && enabled {
                Some(katze_system2::host::graphics::Rect {
                    left: -20,
                    top: 33,
                    right: 1004,
                    bottom: 609,
                })
            } else {
                None
            });
            runtime.drawing_mut().mode = mode;
            runtime.drawing_mut().enabled = enabled;
            *runtime.input_coordinates_mut() = CoordinateMap {
                device_offset: [10.75, -3.25],
                device_scale: 2.,
                configuration_offset: [1, -2],
                logical_scale: [3., -0.5],
                ..Default::default()
            };
            let task = runtime.create(0, kind::ORDINARY, b"exit", b"").unwrap();
            runtime.update(0, 1).unwrap();
            let vm = runtime.object(task).unwrap().interpreter().unwrap();
            assert_eq!(
                vm.read(0, 8).unwrap(),
                expected
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>()
            );
            assert_eq!(
                vm.read(0x80000000, 8).unwrap(),
                external
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>()
            );
            assert_eq!(vm.read(8, 24).unwrap(), [0xaa; 24]);
            assert_eq!(vm.read(0x80000008, 24).unwrap(), [0xbb; 24]);
            assert!(vm.stack_bytes().is_empty());
            assert!(!runtime.drawing().active);
            assert!(runtime.drawing().target.is_none());
        }
    }
}
