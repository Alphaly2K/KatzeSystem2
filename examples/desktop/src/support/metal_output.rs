use katze_system2::{
    Error, Result,
    host::{DrawList, FrameCanvas, GraphicsBackend, RenderTarget},
    session::GameRuntime,
};
use metal::{MetalLayer, MetalLayerRef, foreign_types::ForeignTypeRef, objc::rc::autoreleasepool};
use std::{cell::RefCell, rc::Rc};

#[path = "metal.rs"]
mod renderer;
use renderer::Renderer;

struct View {
    raw: sdl2::sys::SDL_MetalView,
    _window: sdl2::video::Window,
}
impl Drop for View {
    fn drop(&mut self) {
        unsafe { sdl2::sys::SDL_Metal_DestroyView(self.raw) }
    }
}
pub struct Output {
    renderer: Rc<RefCell<Renderer>>,
    layer: MetalLayer,
    _view: View,
}
impl Output {
    pub fn new(window: &sdl2::video::Window) -> Result<Self> {
        let raw = unsafe { sdl2::sys::SDL_Metal_CreateView(window.raw()) };
        if raw.is_null() {
            return Err(Error::new(0, sdl2::get_error()));
        }
        let view = View {
            raw,
            _window: window.clone(),
        };
        let layer = unsafe { sdl2::sys::SDL_Metal_GetLayer(raw) };
        if layer.is_null() {
            return Err(Error::new(0, "窗口缺少 Metal 图层"));
        }
        let layer = unsafe { MetalLayerRef::from_ptr(layer.cast()) }.to_owned();
        let renderer = autoreleasepool(Renderer::new)?;
        renderer.configure_layer(&layer);
        println!("绘制设备：Metal / {}", renderer.device_name());
        Ok(Self {
            renderer: Rc::new(RefCell::new(renderer)),
            layer,
            _view: view,
        })
    }

    pub fn capture_backend(&self) -> impl GraphicsBackend + 'static {
        CaptureBackend(self.renderer.clone())
    }

    pub fn present(&self, runtime: &mut GameRuntime, size: [u32; 2]) -> Result<()> {
        runtime.present_frame(|frame, canvas, capture| {
            let mut renderer = self.renderer.borrow_mut();
            autoreleasepool(|| {
                if !renderer.present(&self.layer, &frame.commands, canvas, size, frame.filter)? {
                    return Ok(());
                }
                if let Some(capture) = capture {
                    let snapshot =
                        renderer.render_offscreen(&frame.commands, canvas, size, frame.filter)?;
                    capture.write_bgra(snapshot.size(), snapshot.stride(), snapshot.pixels())?;
                }
                Ok(())
            })
        })
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        let _ = self.renderer.borrow_mut().wait();
    }
}

// Both passes consume dirty flags from the same canvas and must share its cache.
struct CaptureBackend(Rc<RefCell<Renderer>>);

impl GraphicsBackend for CaptureBackend {
    fn render(
        &mut self,
        frame: &DrawList,
        canvas: &mut FrameCanvas<'_>,
        size: [u32; 2],
    ) -> Result<RenderTarget> {
        self.0.borrow_mut().render(frame, canvas, size)
    }
}
