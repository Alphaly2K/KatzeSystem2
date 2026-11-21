use katze_system2::{
    Result,
    expression::State,
    graphics::Image,
    hg3::{DecodedFrame, StandardInfo},
    host::storage::{FileServices, FolderPaths},
    kcs::{Program, SharedMemory},
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
};
use std::{cell::RefCell, io::Write, rc::Rc};

struct Loader(Program);
impl ProgramLoader for Loader {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        Ok(Some(self.0.clone()))
    }
}

#[derive(Clone, Copy)]
enum Mode {
    Full,
    Missing,
    Zero,
    Short,
    WriteError,
    OpenError,
}
#[derive(Default)]
struct Output {
    names: Vec<Vec<u16>>,
    bytes: Vec<Vec<u8>>,
}
struct Files(Rc<RefCell<Output>>, Mode);
struct Writer(Rc<RefCell<Output>>, Mode);
impl Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.borrow_mut().bytes.push(bytes.to_vec());
        match self.1 {
            Mode::Zero => Ok(0),
            Mode::Short => Ok(1),
            Mode::WriteError => Err(std::io::Error::other("synthetic write failure")),
            Mode::Full => Ok(bytes.len()),
            _ => unreachable!(),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        panic!("image output does not flush the writer")
    }
}
impl FileServices for Files {
    fn create_utf16(&mut self, task: u32, name: &[u16]) -> Result<Option<Box<dyn Write>>> {
        assert_eq!(task, 1);
        self.0.borrow_mut().names.push(name.to_vec());
        match self.1 {
            Mode::Missing => Ok(None),
            Mode::OpenError => Err(katze_system2::Error::new(0, "synthetic service failure")),
            _ => Ok(Some(Box::new(Writer(self.0.clone(), self.1)))),
        }
    }
    fn append_line(&mut self, _: u32, _: &[u8], _: &[u8]) -> Result<()> {
        unreachable!()
    }
}
fn ins(code: &mut Vec<u8>, opcode: u8, values: &[u32]) {
    code.push(opcode);
    for value in values {
        code.push(3);
        code.extend(value.to_le_bytes());
    }
}
fn call(code: &mut Vec<u8>, native: u32, arguments: &[u32], out: u32) {
    for &value in arguments {
        ins(code, 0x25, &[value]);
    }
    ins(code, 0x2b, &[native, arguments.len() as u32 * 4]);
    ins(code, 0x26, &[out]);
}
fn image() -> Image {
    Image::from_frame(DecodedFrame {
        id: 0,
        info: StandardInfo {
            size: [2, 2],
            bit_depth: 32,
            offset: [0; 2],
            canvas_size: [2, 2],
            base: [0; 2],
            reserved: 1,
        },
        pixels: vec![
            10, 20, 30, 255, 40, 50, 60, 128, 70, 80, 90, 0, 100, 110, 120, 1,
        ],
        attributes: Default::default(),
    })
    .unwrap()
}
fn runtime(code: &[u8], data: &[u8], budget: usize) -> Runtime<KcsFactory<Loader>> {
    let mut rt = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        KcsFactory {
            programs: Loader(Program::from_code(code, data.to_vec(), 256).unwrap()),
            external: SharedMemory::new(data.to_vec()).unwrap(),
        },
    );
    rt.set_budget(budget, 1 << 20);
    rt.host_mut().folder_paths = Some(FolderPaths {
        save: "output/資料/🌟".encode_utf16().collect(),
        ..Default::default()
    });
    assert_eq!(rt.images_mut().create(0, [2, 2]).unwrap(), 1);
    *rt.images_mut().get_mut(1).unwrap() = image();
    rt
}
fn word(rt: &Runtime<KcsFactory<Loader>>, task: u32, at: u32) -> u32 {
    let vm = rt.object(task).unwrap().interpreter().unwrap();
    u32::from_le_bytes(vm.read(at, 4).unwrap().as_ref().try_into().unwrap())
}
fn png_pixels(bytes: &[u8]) -> (png::ColorType, Vec<u8>) {
    let mut reader = png::Decoder::new(bytes).read_info().unwrap();
    assert_eq!((reader.info().width, reader.info().height), (2, 2));
    let mut pixels = vec![0; reader.output_buffer_size()];
    let frame = reader.next_frame(&mut pixels).unwrap();
    pixels.truncate(frame.buffer_size());
    (frame.color_type, pixels)
}
fn check_jpeg(bytes: &[u8]) {
    assert_eq!(&bytes[..2], &[255, 216]);
    assert_eq!(&bytes[bytes.len() - 2..], &[255, 217]);
    let mut at = 2;
    let mut baseline = false;
    while at + 4 < bytes.len() {
        assert_eq!(bytes[at], 255);
        let marker = bytes[at + 1];
        let length = u16::from_be_bytes(bytes[at + 2..at + 4].try_into().unwrap()) as usize;
        if marker == 0xc0 {
            assert_eq!(&bytes[at + 4..at + 10], &[8, 0, 2, 0, 2, 3]);
            assert_eq!(
                [bytes[at + 11], bytes[at + 14], bytes[at + 17]],
                [0x22, 0x11, 0x11]
            );
            baseline = true;
        }
        if marker == 0xda {
            break;
        }
        at += 2 + length;
    }
    assert!(baseline);
}

#[test]
fn image_export_encodes_formats_and_keeps_shared_conversion_and_duplicate_swap_state() {
    let names: [&[u8]; 7] = [
        b"%%save%%/a%u[G60].PnG",
        b"%save%/b.JpG",
        b"%save%/c.PNG",
        b"%save%/d.BMP",
        b"%save%/e.unknown",
        b"%save%/f.png",
        b"%save%/g.jpg",
    ];
    for external in [0, 0x8000_0000] {
        for budget in [1, 4096] {
            let mut code = Vec::new();
            let mut data = vec![0; 1024];
            data[60..64].copy_from_slice(&7u32.to_le_bytes());
            ins(&mut code, 0x25, &[0x1357_9bdf]);
            for (i, name) in names.iter().enumerate() {
                let address = 128 + i * 96;
                data[address..address + name.len()].copy_from_slice(name);
                if i == 5 {
                    call(&mut code, 118, &[1], 40);
                }
                if i < 5 {
                    call(
                        &mut code,
                        1009,
                        &[1, external | address as u32],
                        i as u32 * 4,
                    );
                } else {
                    code.extend([0x25, 0x13]);
                    code.extend(40u32.to_le_bytes());
                    ins(&mut code, 0x25, &[external | address as u32]);
                    ins(&mut code, 0x2b, &[1009, 8]);
                    ins(&mut code, 0x26, &[i as u32 * 4]);
                }
            }
            // Exported 24-bit pixels retain their depth through thumbnail records.
            call(&mut code, 238, &[64], 44);
            for native in [241, 251] {
                code.extend([0x25, 0x13]);
                code.extend(44u32.to_le_bytes());
                ins(&mut code, 0x25, &[7]);
                if native == 241 {
                    ins(&mut code, 0x25, &[1]);
                }
                ins(
                    &mut code,
                    0x2b,
                    &[native, if native == 241 { 12 } else { 8 }],
                );
                ins(&mut code, 0x26, &[if native == 241 { 48 } else { 52 }]);
            }
            code.push(0x3b);
            let mut rt = runtime(&code, &data, budget);
            let shared = rt.images_mut().share(1).unwrap();
            let output = Rc::new(RefCell::new(Output::default()));
            rt.set_file_services(Files(output.clone(), Mode::Full));
            let task = rt.create(0, kind::ORDINARY, b"export", b"").unwrap();
            rt.update(0, 0).unwrap();
            for i in 0..7 {
                assert_eq!(word(&rt, task, i * 4), 1);
            }
            let vm = rt.object(task).unwrap().interpreter().unwrap();
            assert_eq!(vm.stack_bytes(), 0x1357_9bdfu32.to_le_bytes());
            assert_eq!(vm.read(0x8000_0000, data.len()).unwrap(), data);
            let output = output.borrow();
            assert_eq!(output.names.len(), 7);
            assert_eq!(
                String::from_utf16(&output.names[0]).unwrap(),
                "output/資料/🌟/a7.PnG"
            );
            assert_eq!(output.bytes.len(), 7);
            assert_eq!(
                png_pixels(&output.bytes[0]),
                (
                    png::ColorType::Rgba,
                    vec![
                        90, 80, 70, 0, 120, 110, 100, 1, 30, 20, 10, 255, 60, 50, 40, 128
                    ]
                )
            );
            let rgb = vec![0, 0, 0, 0, 0, 0, 9, 19, 29, 20, 25, 30];
            assert_eq!(
                png_pixels(&output.bytes[2]),
                (png::ColorType::Rgb, rgb.clone())
            );
            assert_eq!(png_pixels(&output.bytes[5]), (png::ColorType::Rgb, rgb));
            for i in [1, 4, 6] {
                check_jpeg(&output.bytes[i]);
            }
            let bmp = &output.bytes[3];
            assert_eq!(&bmp[..2], b"BM");
            assert_eq!(u32::from_le_bytes(bmp[2..6].try_into().unwrap()), 70);
            assert_eq!(bmp.len(), 72);
            assert_eq!(&bmp[18..30], &[2, 0, 0, 0, 2, 0, 0, 0, 1, 0, 24, 0]);
            assert_eq!(
                &bmp[54..70],
                &[0, 0, 0, 0, 0, 0, 0, 0, 29, 19, 9, 30, 25, 20, 0, 0]
            );
            let source = rt.images().get(shared).unwrap();
            assert_eq!(source.info().bit_depth, 24);
            assert_eq!(
                source.surface().pixels(),
                &[29, 19, 9, 255, 30, 25, 20, 255, 0, 0, 0, 255, 0, 0, 0, 255]
            );
            let duplicate = rt.images().get(word(&rt, task, 40)).unwrap();
            assert_eq!(
                duplicate.surface().pixels(),
                &[9, 19, 29, 255, 20, 25, 30, 255, 0, 0, 0, 255, 0, 0, 0, 255]
            );
            assert_eq!(word(&rt, task, 48), 1);
            let restored = rt.images().get(word(&rt, task, 52)).unwrap();
            assert_eq!(restored.info().bit_depth, 24);
            assert_eq!(restored.surface().pixels(), source.surface().pixels());
        }
    }
}

#[test]
fn image_export_keeps_preprocessing_after_open_or_single_write_failures() {
    for mode in [
        Mode::Missing,
        Mode::Zero,
        Mode::Short,
        Mode::WriteError,
        Mode::OpenError,
    ] {
        let mut code = Vec::new();
        call(&mut code, 1009, &[1, 64], 0);
        code.push(0x3b);
        let mut data = vec![0; 128];
        data[64..71].copy_from_slice(b"out.bmp");
        let mut rt = runtime(&code, &data, 1);
        let output = Rc::new(RefCell::new(Output::default()));
        rt.set_file_services(Files(output.clone(), mode));
        let task = rt.create(0, kind::ORDINARY, b"failures", b"").unwrap();
        let result = rt.update(0, 0);
        if matches!(mode, Mode::OpenError) {
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("synthetic service failure")
            );
        } else {
            result.unwrap();
            assert_eq!(
                word(&rt, task, 0),
                u32::from(matches!(mode, Mode::Full | Mode::Short | Mode::WriteError))
            );
        }
        let output = output.borrow();
        assert_eq!(output.names.len(), 1);
        assert_eq!(
            output.bytes.len(),
            usize::from(!matches!(mode, Mode::Missing | Mode::OpenError))
        );
        let image = rt.images().get(1).unwrap();
        assert_eq!(image.info().bit_depth, 24);
        assert_eq!(
            image.surface().pixels(),
            &[9, 19, 29, 255, 20, 25, 30, 255, 0, 0, 0, 255, 0, 0, 0, 255]
        );
    }
}

#[test]
fn null_image_export_skips_name_directory_and_file_service_access() {
    let mut code = Vec::new();
    call(&mut code, 1009, &[0, u32::MAX], 0);
    code.push(0x3b);
    let mut rt = runtime(&code, &[0; 64], 1);
    rt.host_mut().folder_paths = None;
    let task = rt.create(0, kind::ORDINARY, b"null", b"").unwrap();
    rt.update(0, 0).unwrap();
    assert_eq!(word(&rt, task, 0), 0);
    assert!(
        rt.object(task)
            .unwrap()
            .interpreter()
            .unwrap()
            .stack_bytes()
            .is_empty()
    );
}
