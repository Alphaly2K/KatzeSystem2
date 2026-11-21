use katze_system2::{
    Error, Result,
    expression::State,
    graphics::{Projection, RenderTarget, Transform},
    host::{
        graphics::{Surface, TextureFilter, software::SoftwareBackend},
        storage::FileServices,
    },
    kcs::{CoreHost, Host, Interpreter, NativeReturn, Program},
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind},
};
use std::{cell::RefCell, io::Write, rc::Rc};

struct Loader(Program);
impl ProgramLoader for Loader {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        Ok(Some(self.0.clone()))
    }
}
type Engine = Runtime<KcsFactory<Loader>>;
fn ins(code: &mut Vec<u8>, op: u8, args: &[u32]) {
    code.push(op);
    for arg in args {
        code.push(3);
        code.extend(arg.to_le_bytes());
    }
}
fn engine(mode: u32, index: u32, folder: &[u8], budget: usize) -> Engine {
    let mut code = Vec::new();
    ins(&mut code, 0x25, &[0x13579bdf]);
    for value in [mode, 32, index] {
        ins(&mut code, 0x25, &[value]);
    }
    ins(&mut code, 0x2b, &[441, 12]);
    ins(&mut code, 0x26, &[0]);
    code.push(0x3b);
    let mut data = vec![0; 256];
    data[32..32 + folder.len()].copy_from_slice(folder);
    let mut rt = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        KcsFactory::new(Loader(Program::from_code(&code, data, 64).unwrap())),
    );
    rt.set_budget(budget, 4096);
    let id = rt.create(0, kind::ORDINARY, b"capture", b"").unwrap();
    for _ in 0..32 {
        rt.update(0, 1).unwrap();
        if rt.object(id).unwrap().interpreter().unwrap().pc == code.len() {
            break;
        }
    }
    assert_eq!(
        rt.object(id)
            .unwrap()
            .interpreter()
            .unwrap()
            .read(0, 4)
            .unwrap(),
        0x13579bdfu32.to_le_bytes()
    );
    assert_eq!(rt.host().frame_capture_enabled, mode);
    assert_eq!(rt.host().frame_capture_index, index);
    rt.drawing_mut().enabled = true;
    rt.drawing_mut().target = Some(RenderTarget::new([3, 2]).unwrap());
    rt.set_frame_view(
        Projection::default(),
        *Transform::default().matrix(),
        TextureFilter::Point,
    );
    rt
}
fn frame() -> RenderTarget {
    RenderTarget::from_tiles(
        [3, 2],
        [1, 1],
        vec![Some(
            Surface::from_bgra(
                [3, 2],
                vec![
                    10, 20, 30, 0, 40, 50, 60, 1, 70, 80, 90, 127, 110, 120, 130, 128, 140, 150,
                    160, 254, 170, 180, 190, 255,
                ],
            )
            .unwrap(),
        )],
    )
    .unwrap()
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
#[derive(Default, Debug)]
struct Output {
    names: Vec<Vec<u16>>,
    bytes: Vec<Vec<u8>>,
}
struct Files(Rc<RefCell<Output>>, Mode);
struct Writer(Rc<RefCell<Output>>, Mode);
impl FileServices for Files {
    fn create_utf16(&mut self, task: u32, name: &[u16]) -> Result<Option<Box<dyn Write>>> {
        assert_eq!(task, 1);
        self.0.borrow_mut().names.push(name.to_vec());
        match self.1 {
            Mode::Missing => Ok(None),
            Mode::OpenError => Err(Error::new(0, "synthetic service failure")),
            mode => Ok(Some(Box::new(Writer(self.0.clone(), mode)))),
        }
    }
    fn append_line(&mut self, _: u32, _: &[u8], _: &[u8]) -> Result<()> {
        unreachable!()
    }
}
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
        panic!("capture does not flush")
    }
}
fn output(rt: &mut Engine, mode: Mode) -> Rc<RefCell<Output>> {
    let result = Rc::new(RefCell::new(Output::default()));
    rt.host_mut().file_services = Some(Box::new(Files(result.clone(), mode)));
    result
}

#[test]
fn configuration_formats_cp932_prefix_and_rejects_buffer_overflow_before_changing_state()
-> Result<()> {
    for budget in [1, 100] {
        let rt = engine(u32::MAX, 0x80000000, b"\x82\xa0_%u[G16]", budget);
        assert_eq!(
            rt.host().frame_capture_folder,
            "あ_0".encode_utf16().collect::<Vec<_>>()
        );
        assert!(rt.host().window_requests.is_empty());
    }
    let mut state = State::new(0);
    let mut host = CoreHost::new(&mut state);
    let mut vm = Interpreter::new(Program::from_code(&[], vec![0; 256], 16)?, vec![])?;
    vm.write(0, b"\x81\x7f\0")?;
    let args = [0u32, 0, 17]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    assert_eq!(host.call(441, &args, &mut vm)?, NativeReturn::Continue);
    assert_eq!(host.frame_capture_folder, [0x30fb]);
    assert_eq!(host.call(565, &[], &mut vm)?, NativeReturn::Value(0));
    assert_eq!(host.call(566, &[], &mut vm)?, NativeReturn::Value(17));
    vm.write(0, &[vec![b'x'; 49], vec![0]].concat())?;
    host.call(441, &args, &mut vm)?;
    let before = host.frame_capture_folder.clone();
    vm.write(0, &[vec![b'x'; 50], vec![0]].concat())?;
    let overflow = [1u32, 0, 99]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    assert!(host.call(441, &overflow, &mut vm).is_err());
    assert!(host.call(441, &args[..8], &mut vm).is_err());
    assert_eq!(host.frame_capture_folder, before);
    assert_eq!(host.frame_capture_index, 17);
    assert_eq!(host.frame_capture_enabled, 0);
    Ok(())
}

#[test]
fn sequence_output_keeps_rgb_orientation_signed_indices_and_consumes_failed_writes() -> Result<()> {
    for mode in [
        Mode::Full,
        Mode::Missing,
        Mode::Zero,
        Mode::Short,
        Mode::WriteError,
        Mode::OpenError,
    ] {
        let mut rt = engine(1, u32::MAX, b"%save%", 1);
        let out = output(&mut rt, mode);
        let frame = frame();
        let before = frame.clone();
        let result = rt.capture_sequence_frame(&frame);
        assert_eq!(result.is_err(), matches!(mode, Mode::OpenError));
        assert_eq!(rt.host().frame_capture_index, 0);
        assert_eq!(frame, before);
        let out = out.borrow();
        assert_eq!(
            out.names,
            ["%save%\\%save%_-000000001.bmp"
                .encode_utf16()
                .collect::<Vec<_>>()]
        );
        if !matches!(mode, Mode::Missing | Mode::OpenError) {
            assert_eq!(out.bytes.len(), 1);
            let bytes = &out.bytes[0];
            assert_eq!(&bytes[..2], b"BM");
            assert_eq!(bytes.len(), 80);
            assert_eq!(&bytes[18..30], &[3, 0, 0, 0, 2, 0, 0, 0, 1, 0, 24, 0]);
            assert_eq!(
                &bytes[54..],
                &[
                    110, 120, 130, 140, 150, 160, 170, 180, 190, 0, 0, 0, 10, 20, 30, 40, 50, 60,
                    70, 80, 90, 0, 0, 0, 0, 0
                ]
            );
        } else {
            assert!(out.bytes.is_empty());
        }
    }
    let mut rt = engine(1, 7, b"dump", 100);
    rt.drawing_mut().enabled = false;
    assert!(!rt.capture_sequence_frame(&frame())?);
    rt.drawing_mut().enabled = true;
    rt.drawing_mut().mode = 1;
    assert!(!rt.capture_sequence_frame(&frame())?);
    rt.drawing_mut().mode = 0;
    assert!(!rt.capture_sequence_frame(&RenderTarget::new([0, 0])?)?);
    assert_eq!(rt.host().frame_capture_index, 7);
    assert!(rt.capture_sequence_frame(&frame()).is_err());
    assert_eq!(rt.host().frame_capture_index, 8);
    let out = output(&mut rt, Mode::Full);
    rt.set_frame_capture_rectangle(Some(katze_system2::host::graphics::Rect::from_xywh(
        1, 0, 2, 2,
    )));
    assert!(rt.capture_sequence_frame(&frame())?);
    let out = out.borrow();
    assert_eq!(&out.bytes[0][18..26], &[2, 0, 0, 0, 2, 0, 0, 0]);
    assert_eq!(
        &out.bytes[0][54..70],
        &[
            140, 150, 160, 170, 180, 190, 0, 0, 40, 50, 60, 70, 80, 90, 0, 0
        ]
    );
    assert!(!rt.capture_sequence_frame(&RenderTarget::new([0, 0])?)?);
    let missing = RenderTarget::from_tiles([3, 2], [1, 1], vec![None])?;
    assert!(rt.capture_sequence_frame(&missing).is_err());
    assert_eq!(rt.host().frame_capture_index, 9);
    Ok(())
}

#[test]
fn sequence_capture_request_tells_the_host_when_to_read_back() -> Result<()> {
    let mut rt = engine(1, 9, b"dump", 100);
    let out = output(&mut rt, Mode::Full);
    for (capture, mode, drawing, expected) in [
        (0, 0, true, false),
        (1, 1, true, false),
        (1, 0, false, false),
        (1, 0, true, true),
    ] {
        rt.host_mut().frame_capture_enabled = capture;
        rt.drawing_mut().mode = mode;
        rt.drawing_mut().enabled = drawing;
        assert_eq!(rt.wants_sequence_capture(), expected);
        rt.present_frame(|_, canvas, capture| {
            assert_eq!(canvas.size(), [3, 2]);
            assert_eq!(capture.is_some(), expected);
            Ok(())
        })?;
        if !expected {
            assert!(!rt.capture_sequence_bgra([3, 2], 0, &[])?);
        }
    }
    assert!(
        rt.present_frame::<()>(|_, _, capture| {
            assert!(capture.is_some());
            Err(Error::new(0, "synthetic presentation failure"))
        })
        .is_err()
    );
    let frame = frame();
    let surface = frame.tiles()[0].as_ref().unwrap();
    let pixels = surface.pixels();
    let mut padded = pixels[..12].to_vec();
    padded.extend([0xac; 4]);
    padded.extend(&pixels[12..]);
    assert!(
        rt.present_frame(|_, _, capture| capture.unwrap().write_bgra([3, 2], 11, pixels))
            .is_err()
    );
    assert!(rt.capture_sequence_bgra([3, 2], 16, &padded[..27]).is_err());
    assert!(
        rt.capture_sequence_bgra([3, 2], usize::MAX, &padded)
            .is_err()
    );
    assert_eq!(rt.host().frame_capture_index, 9);
    assert!(out.borrow().bytes.is_empty());
    rt.set_frame_capture_rectangle(Some(katze_system2::host::graphics::Rect::from_xywh(
        -1, 0, 3, 2,
    )));
    assert!(rt.present_frame(|_, _, capture| {
        SoftwareBackend::submit_capture(capture.unwrap(), surface)
    })?);
    assert!(rt.present_frame(|_, _, capture| capture.unwrap().write_bgra([3, 2], 16, &padded))?);
    assert_eq!(rt.host().frame_capture_index, 11);
    let out = out.borrow();
    assert_eq!(out.bytes.len(), 2);
    assert_eq!(out.bytes[0], out.bytes[1]);
    Ok(())
}
