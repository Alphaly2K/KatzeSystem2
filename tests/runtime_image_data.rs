use katze_system2::{
    Result,
    expression::State,
    graphics::Image,
    hg3::{DecodedFrame, StandardInfo},
    kcs::Program,
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
};
struct Loader(Program);
impl ProgramLoader for Loader {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        Ok(Some(self.0.clone()))
    }
}
fn call(code: &mut Vec<u8>, native: u32, args: &[u32], output: Option<u32>) {
    for &value in args {
        code.extend([0x25, 3]);
        code.extend(value.to_le_bytes());
    }
    code.extend([0x2b, 3]);
    code.extend(native.to_le_bytes());
    code.push(3);
    code.extend((args.len() as u32 * 4).to_le_bytes());
    if let Some(output) = output {
        code.extend([0x26, 3]);
        code.extend(output.to_le_bytes());
    }
}

#[test]
fn raw_image_geometry_and_pixels_keep_bottom_coordinates_overlap_and_shared_storage() {
    let mut code = Vec::new();
    call(&mut code, 363, &[1, 80, 84], None);
    call(&mut code, 362, &[1, 88, 92], None);
    call(&mut code, 1026, &[1, (-2i32) as u32, 5], Some(0));
    call(&mut code, 1026, &[1, (-2i32) as u32, 6], Some(4));
    call(&mut code, 1026, &[1, 1, 5], Some(8));
    call(&mut code, 372, &[1, 40, 0], Some(12));
    call(&mut code, 362, &[1, 96, 100], None);
    call(&mut code, 373, &[1, 10, (-3i32) as u32], Some(16));
    call(&mut code, 363, &[1, 104, 105], None);
    call(&mut code, 372, &[0, u32::MAX, u32::MAX], Some(20));
    call(&mut code, 373, &[0, u32::MAX, u32::MAX], Some(24));
    call(&mut code, 369, &[0, 0], None);
    call(&mut code, 1026, &[0, 0, 0], Some(28));
    call(&mut code, 369, &[1, 0x7f808182], None);
    call(&mut code, 1026, &[1, 10, (-3i32) as u32], Some(32));
    call(&mut code, 1026, &[1, 10, (-2i32) as u32], Some(36));
    code.push(0x3b);
    let program = Program::from_code(&code, vec![0xa5; 128], 128).unwrap();
    for budget in [1, 4096] {
        let mut rt = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            KcsFactory::new(Loader(program.clone())),
        );
        rt.set_budget(budget, 1 << 20);
        let image = rt.images_mut().create(0, [3, 2]).unwrap();
        assert_eq!(image, 1);
        let frame = DecodedFrame {
            id: 7,
            info: StandardInfo {
                size: [3, 2],
                bit_depth: 32,
                offset: [-2, 5],
                canvas_size: [0, 20],
                reserved: 0,
                base: [8, 9],
            },
            pixels: [
                0xff010203u32,
                0x804080c0,
                0x12345678,
                0xfff0e0d0,
                0x01020304,
                0x00112233,
            ]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect(),
            attributes: Default::default(),
        };
        *rt.images_mut().get_mut(image).unwrap() = Image::from_frame(frame).unwrap();
        let shared = rt.images_mut().share(image).unwrap();
        let task = rt.create(0, kind::ORDINARY, b"main", b"").unwrap();
        rt.update(0, 0).unwrap();
        let vm = rt.object(task).unwrap().interpreter().unwrap();
        let word = |at| u32::from_le_bytes(vm.read(at, 4).unwrap().as_ref().try_into().unwrap());
        let results: Vec<_> = (0..10).map(|i| word(i * 4)).collect();
        assert_eq!(
            results,
            [
                0xfff0e0d0, 0xff010203, 0, 1, 1, 0, 0, 0, 0x7f78716a, 0x7f000101
            ]
        );
        assert_eq!(
            [word(80), word(84), word(88), word(92), word(96), word(100)],
            [(-2i32) as u32, 5, 3, 20, 40, 2]
        );
        assert_eq!(vm.read(104, 5).unwrap(), [10, 0xfd, 0xff, 0xff, 0xff]);
        assert!(vm.stack_bytes().is_empty());
        let other = rt.images().get(shared).unwrap();
        assert_eq!(other.info().offset, [10, -3]);
        assert_eq!(other.info().canvas_size, [40, 0]);
        assert_eq!(other.info().base, [8, 9]);
        assert_eq!(other.surface().pixel(0, 0), Some(0x7f000101));
        assert_eq!(other.surface().pixel(0, 1), Some(0x7f78716a));
    }
}
