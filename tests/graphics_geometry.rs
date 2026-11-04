use katze_system2::{
    Error, Result,
    expression::State,
    graphics::{
        Drawable, Drawables, DrawingContext, GraphicsFactory, PlaneObject, Projection, RenderTarget,
    },
    kcs::{Host, Interpreter, NativeReturn, Program, ResourceServices},
    resource::ResourceMode,
    runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, TaskObject, kind},
    task::{Command, CommandBuffers, CommandOutcome},
};
use std::{cell::RefCell, rc::Rc};

fn bytes(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn geometry(bounds: [u32; 4], triangles: &[[u32; 12]]) -> Vec<u8> {
    let mut data = b"KX2\0".to_vec();
    data.extend(bytes(&bounds));
    data.extend((triangles.len() as u32).to_le_bytes());
    for triangle in triangles {
        data.extend(bytes(triangle));
    }
    data
}

fn mesh() -> Vec<u8> {
    geometry(
        [
            (-7f32).to_bits(),
            9f32.to_bits(),
            3f32.to_bits(),
            (-5f32).to_bits(),
        ],
        &[[
            0,
            0,
            0,
            0x801234e0,
            4f32.to_bits(),
            0,
            0,
            0x801234e0,
            0,
            4f32.to_bits(),
            0,
            0x801234e0,
        ]],
    )
}

#[test]
fn kx2_keeps_declared_bounds_raw_vertices_and_checked_record_lengths() {
    let bounds = [0x80000000, 0x7fc12345, 0x3f800001, 0xff800000];
    let triangle = [
        0x80000000,
        0x7fc54321,
        0xff800000,
        0x00123456,
        0x3f800000,
        0,
        0,
        0x80abcdef,
        0,
        0x3f800000,
        0,
        u32::MAX,
    ];
    let mut data = geometry(bounds, &[triangle]);
    data.extend(b"trailing bytes");
    let drawable = Drawable::from_kx2(&data).unwrap();
    assert_eq!(
        drawable.bounds().map(|p| p.map(f32::to_bits)),
        [[bounds[0], bounds[1], 0], [bounds[2], bounds[3], 0]]
    );
    for (vertex, words) in drawable.triangles()[0].iter().zip(triangle.chunks_exact(4)) {
        assert_eq!(vertex.position.map(f32::to_bits), words[..3]);
        assert_eq!(vertex.color, words[3]);
    }
    assert_eq!(drawable.display(), 0);
    assert_eq!(drawable.color(), (u32::MAX, 0));
    assert!(
        Drawable::from_kx2(&geometry(bounds, &[]))
            .unwrap()
            .triangles()
            .is_empty()
    );
    for length in [0, 4, 20, 24, 24 + 47] {
        assert!(Drawable::from_kx2(&data[..length]).is_err());
    }
    let mut oversized = geometry(bounds, &[]);
    oversized[20..24].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(
        Drawable::from_kx2(&oversized)
            .unwrap_err()
            .message
            .contains("上限")
    );
    oversized[0] = b'x';
    assert!(
        Drawable::from_kx2(&oversized)
            .unwrap_err()
            .message
            .contains("无效 KX2")
    );
}

type Requests = Rc<RefCell<Vec<(bool, Vec<u8>)>>>;

struct Files {
    data: Option<Vec<u8>>,
    requests: Requests,
}

impl ResourceServices for Files {
    fn resolve(&mut self, category: &[u8], name: &[u8]) -> Result<Vec<u8>> {
        assert_eq!(category, b"kx2");
        self.requests.borrow_mut().push((true, name.to_vec()));
        Ok(b"geometry/mesh.kx2".to_vec())
    }

    fn read(&mut self, name: &[u8], mode: ResourceMode) -> Result<Option<Vec<u8>>> {
        assert_eq!(mode, ResourceMode::LooseFirst);
        self.requests.borrow_mut().push((false, name.to_vec()));
        Ok(self.data.clone())
    }
}

struct GeometryHost {
    drawables: Drawables,
    files: Files,
}

impl Host for GeometryHost {
    fn call(&mut self, _: u32, _: &[u8], _: &mut Interpreter) -> Result<NativeReturn> {
        Err(Error::new(0, "合成宿主不处理 native"))
    }

    fn drawables(&mut self) -> Option<&mut Drawables> {
        Some(&mut self.drawables)
    }

    fn resource_services(&mut self) -> Option<&mut dyn ResourceServices> {
        Some(&mut self.files)
    }
}

fn send(
    plane: &mut PlaneObject,
    host: &mut GeometryHost,
    code: u32,
    value: u32,
    buffer: Option<&mut [u8]>,
) -> Result<()> {
    let mut output = 0xdeadbeef;
    assert_eq!(
        plane.dispatch_command_with_buffers(
            kind::PLANE,
            &Command {
                target: 7,
                sender: 99,
                code,
                data: [0, value, 0],
            },
            &mut CommandBuffers {
                data0: None,
                data1: buffer,
                external: None,
            },
            &mut output,
            host,
            1
        )?,
        CommandOutcome::finished(false)
    );
    assert_eq!(output, 0xdeadbeef);
    Ok(())
}

#[test]
fn plane_geometry_replacement_preserves_failed_metadata_and_resource_read_gates() {
    let mut host = GeometryHost {
        drawables: Drawables::default(),
        files: Files {
            data: Some(mesh()),
            requests: Rc::default(),
        },
    };
    let mut plane = PlaneObject::new(7);
    send(
        &mut plane,
        &mut host,
        70,
        0,
        Some(&mut b"mesh.kx2\0".to_vec()),
    )
    .unwrap();
    assert_eq!(plane.shape_kind(), 7);
    assert_eq!(plane.geometry_name(), b"mesh.kx2");
    assert_eq!(plane.offset(), [-7., 9.]);
    assert_eq!(plane.size(), [10., 14.]);
    assert_eq!(
        host.drawables.get(7).unwrap().triangles()[0][0].position,
        [0.; 3]
    );
    host.drawables.set_position(7, [3., 4., 5.]);
    host.files.data = None;
    send(
        &mut plane,
        &mut host,
        70,
        0,
        Some(&mut b"missing\0".to_vec()),
    )
    .unwrap();
    assert!(host.drawables.get(7).is_none());
    assert_eq!(plane.size(), [10., 14.]);
    assert_eq!(plane.offset(), [-7., 9.]);
    assert_eq!(plane.shape_kind(), 7);
    assert_eq!(plane.geometry_name(), b"mesh.kx2");
    host.files.data = Some(b"unsupported".to_vec());
    send(&mut plane, &mut host, 70, 0, Some(&mut b"wrong\0".to_vec())).unwrap();
    assert!(host.drawables.get(7).is_none());
    host.files.data = Some(mesh()[..30].to_vec());
    assert!(send(&mut plane, &mut host, 70, 0, Some(&mut b"short\0".to_vec())).is_err());
    for flags in [0, 2, u32::MAX] {
        send(&mut plane, &mut host, 84, flags, None).unwrap();
        send(&mut plane, &mut host, 70, 0, Some(&mut b"skip\0".to_vec())).unwrap();
    }
    assert_eq!(host.files.requests.borrow().len(), 4);
    host.files.data = Some(geometry([0; 4], &[]));
    send(&mut plane, &mut host, 84, 1, None).unwrap();
    send(
        &mut plane,
        &mut host,
        70,
        0,
        Some(&mut b"empty.kx2\0".to_vec()),
    )
    .unwrap();
    assert_eq!(plane.size(), [0.; 2]);
    assert_eq!(
        host.drawables.get(7).unwrap().transform().position(),
        [0.; 3]
    );
    send(&mut plane, &mut host, 68, 0, None).unwrap();
    assert_eq!(plane.geometry_name(), b"empty.kx2");
    assert!(
        send(
            &mut plane,
            &mut host,
            70,
            0,
            Some(&mut b"no terminator".to_vec())
        )
        .is_err()
    );
    send(&mut plane, &mut host, 3, 0, None).unwrap();
    send(&mut plane, &mut host, 70, 0, None).unwrap();
    assert_eq!(host.files.requests.borrow().len(), 5);
}

struct Programs(Program);
impl ProgramLoader for Programs {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        Ok(Some(self.0.clone()))
    }
}

fn ins(code: &mut Vec<u8>, op: u8, args: &[u32]) {
    code.push(op);
    for arg in args {
        code.push(3);
        code.extend(arg.to_le_bytes());
    }
}

#[test]
fn ordinary_kx2_load_formats_resolves_routes_and_reaches_cpu_pixels() {
    let mut code = vec![];
    ins(&mut code, 0x25, &[0xdeadbeef]);
    for args in [&[0u32][..], &[99, 0], &[1, 0], &[1, 32]] {
        for arg in args {
            ins(&mut code, 0x25, &[*arg]);
        }
        ins(&mut code, 0x2b, &[82, args.len() as u32 * 4]);
    }
    ins(&mut code, 0x26, &[40]);
    ins(&mut code, 0x3b, &[]);
    let mut data = vec![0; 64];
    data[..11].copy_from_slice(b"mesh%u[G16]");
    data[16..20].copy_from_slice(&7u32.to_le_bytes());
    let program = Program::from_code(&code, data, 64).unwrap();
    for budget in [1, 4096] {
        let requests = Requests::default();
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(0),
            GraphicsFactory {
                fallback: KcsFactory::new(Programs(program.clone())),
            },
        );
        runtime.set_budget(budget, katze_system2::runtime::DEFAULT_LIMIT);
        let plane = runtime.create(0, kind::PLANE, b"plane", b"").unwrap();
        let task = runtime.create(0, kind::ORDINARY, b"load", b"").unwrap();
        runtime.set_resources(Files {
            data: Some(mesh()),
            requests: requests.clone(),
        });
        runtime.update(1, 1).unwrap();
        assert_eq!(
            *requests.borrow(),
            [
                (true, b"mesh7".to_vec()),
                (true, b"mesh7".to_vec()),
                (false, b"geometry/mesh.kx2".to_vec()),
                (true, vec![]),
                (false, b"geometry/mesh.kx2".to_vec()),
            ]
        );
        assert_eq!(
            runtime
                .object(task)
                .unwrap()
                .interpreter()
                .unwrap()
                .read(40, 4)
                .unwrap(),
            0xdeadbeefu32.to_le_bytes()
        );
        assert_eq!(
            runtime.drawables().get(plane).unwrap().bounds(),
            [[-7., 9., 0.], [3., -5., 0.]]
        );
        runtime.drawables_mut().set_display(plane, 1);
        *runtime.drawing_mut() = DrawingContext {
            mode: 0,
            enabled: true,
            active: true,
            target: Some(RenderTarget::new([4, 4]).unwrap()),
        };
        let identity = std::array::from_fn(|i| if i % 5 == 0 { 1. } else { 0. });
        assert!(
            runtime
                .render_triangles(&Projection::default(), identity)
                .unwrap()
        );
        let target = runtime.drawing().target.as_ref().unwrap();
        assert_eq!(target.pixel(0, 0), Some(0x40091a70));
        assert_eq!(target.pixel(3, 3), Some(0));
        runtime.destroy(plane).unwrap();
        assert!(runtime.drawables().get(plane).is_none());
    }
}
