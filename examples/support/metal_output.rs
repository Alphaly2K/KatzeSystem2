use katze_system2::{
    Error, Result,
    graphics::{DrawList, GraphicsBackend, RenderTarget, metal::Renderer},
};
use metal::{MetalLayerRef, foreign_types::ForeignTypeRef};

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
    // 先等待 GPU 提交结束，再释放视图及其持有的窗口。
    renderer: Renderer,
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
        let mut renderer = metal::objc::rc::autoreleasepool(Renderer::new)?;
        renderer.set_presentation_layer(unsafe { MetalLayerRef::from_ptr(layer.cast()) })?;
        println!("绘制设备：Metal / {}", renderer.device_name());
        Ok(Self {
            renderer,
            _view: view,
        })
    }
}
impl GraphicsBackend for Output {
    fn render(
        &mut self,
        frame: &DrawList,
        canvas: &mut RenderTarget,
        size: [u32; 2],
    ) -> Result<RenderTarget> {
        GraphicsBackend::render(&mut self.renderer, frame, canvas, size)
    }
    fn present(
        &mut self,
        frame: &DrawList,
        canvas: &mut RenderTarget,
        size: [u32; 2],
    ) -> Result<bool> {
        GraphicsBackend::present(&mut self.renderer, frame, canvas, size)
    }
}
