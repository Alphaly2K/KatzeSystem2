use katze_system2::{
    Error, Result,
    expression::State,
    graphics::{GraphicsFactory, Projection, RenderTarget, TextureFilter},
    runtime::{BaseObject, ObjectFactory, Runtime, TaskObject, kind},
    task::{Command, CommandBuffers},
};

struct Empty;

impl ObjectFactory for Empty {
    fn create(&mut self, _: u32, _: u32, _: &[u8]) -> Result<Box<dyn TaskObject>> {
        Err(Error::new(0, "合成工厂没有该类型"))
    }
}

type Engine = Runtime<GraphicsFactory<Empty>>;

fn command(target: u32, code: u32, value: u32) -> Command {
    Command {
        target,
        sender: 0,
        code,
        data: [0, value, 0],
    }
}

fn write(runtime: &mut Engine, target: u32, code: u32, words: &[u32]) {
    let mut bytes: Vec<_> = words.iter().flat_map(|value| value.to_le_bytes()).collect();
    runtime
        .dispatch_command_with_buffers(
            0,
            command(target, code, 0),
            &mut CommandBuffers {
                data0: None,
                data1: Some(&mut bytes),
            },
        )
        .unwrap();
}

fn rectangle(runtime: &mut Engine, plane: u32) {
    write(
        runtime,
        plane,
        42,
        &[0, 0, 0, 2f32.to_bits(), 2f32.to_bits(), 0, 0, 0xffe0_4020],
    );
    runtime.drawables_mut().set_display(plane, 1);
}

fn frame(runtime: &mut Engine) -> RenderTarget {
    let mut output = RenderTarget::new([8, 8]).unwrap();
    assert!(
        runtime
            .render_frame(
                &mut output,
                &Projection::default(),
                *katze_system2::graphics::Transform::default().matrix(),
                TextureFilter::Point,
            )
            .unwrap()
    );
    output
}

#[test]
fn camera_binding_projects_pixels_and_survives_until_actual_task_release() {
    let mut runtime = Runtime::new(
        0,
        kind::ROOT,
        Box::new(BaseObject::new()),
        State::new(0),
        GraphicsFactory { fallback: Empty },
    );
    runtime.drawing_mut().target = Some(RenderTarget::new([8, 8]).unwrap());
    let camera = runtime.create(0, kind::CAMERA_2D, b"camera", b"").unwrap();
    let handle = runtime
        .dispatch_command(0, command(camera, 589, 0))
        .unwrap();
    assert_eq!(handle, 1);
    let plane = runtime.create(0, kind::PLANE, b"plane", b"").unwrap();
    runtime
        .dispatch_command(0, command(plane, 591, camera))
        .unwrap();
    rectangle(&mut runtime, plane);
    assert_eq!(runtime.drawables().get(plane).unwrap().camera_id(), 0);
    runtime
        .dispatch_command(0, command(plane, 591, camera))
        .unwrap();
    write(
        &mut runtime,
        camera,
        51,
        &[(-3f32).to_bits(), (-2f32).to_bits()],
    );
    write(&mut runtime, camera, 66, &[2f32.to_bits(), 1f32.to_bits()]);
    let output = frame(&mut runtime);
    assert_eq!(output.pixel(3, 2), Some(0xffe0_4020));
    assert_eq!(output.pixel(6, 3), Some(0xffe0_4020));
    assert_eq!(output.pixel(7, 3), Some(0));
    let mut matrix = Projection::default().coordinate_matrix;
    matrix[12] = 1.;
    write(&mut runtime, camera, 57, &matrix.map(f32::to_bits));
    let output = frame(&mut runtime);
    assert_eq!(output.pixel(3, 2), Some(0));
    assert_eq!(output.pixel(7, 3), Some(0xffe0_4020));

    runtime
        .dispatch_command(0, command(camera, 590, 0))
        .unwrap();
    assert_eq!(frame(&mut runtime).pixel(0, 0), Some(0xffe0_4020));
    write(
        &mut runtime,
        camera,
        51,
        &[(-3f32).to_bits(), (-2f32).to_bits()],
    );
    runtime.dispatch_command(0, command(camera, 3, 0)).unwrap();
    assert!(runtime.drawables().camera(handle).is_some());
    runtime.dispatch_command(0, command(camera, 51, 0)).unwrap();
    assert_eq!(frame(&mut runtime).pixel(3, 2), Some(0xffe0_4020));
    runtime.update(1, 1).unwrap();
    assert!(runtime.drawables().camera(handle).is_none());
    assert_eq!(runtime.drawables().get(plane).unwrap().camera_id(), handle);
    assert_eq!(frame(&mut runtime).pixel(0, 0), Some(0xffe0_4020));

    let second = runtime.create(0, kind::CAMERA_2D, b"second", b"").unwrap();
    let second_handle = runtime
        .dispatch_command(0, command(second, 589, 0))
        .unwrap();
    assert_eq!(second_handle, 2);
    write(
        &mut runtime,
        second,
        51,
        &[(-5f32).to_bits(), (-5f32).to_bits()],
    );
    assert_eq!(frame(&mut runtime).pixel(0, 0), Some(0xffe0_4020));
    runtime
        .dispatch_command(0, command(plane, 591, second))
        .unwrap();
    assert_eq!(frame(&mut runtime).pixel(5, 5), Some(0xffe0_4020));
    runtime.dispatch_command(0, command(plane, 592, 0)).unwrap();
    assert_eq!(runtime.drawables().get(plane).unwrap().camera_id(), 0);
    runtime
        .dispatch_command(0, command(plane, 591, second))
        .unwrap();
    rectangle(&mut runtime, plane);
    assert_eq!(runtime.drawables().get(plane).unwrap().camera_id(), 0);
    runtime.destroy(second).unwrap();
    assert!(runtime.drawables().camera(second_handle).is_none());
}
