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
            factory.external_bytes = 4;
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
