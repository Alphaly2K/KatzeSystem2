use katze_system2::{graphics::RenderTarget, host::platform::WindowServices};
use sdl2::{
    event::Event,
    messagebox::{ButtonData, ClickedButton, MessageBoxButtonFlag, MessageBoxFlag},
    pixels::PixelFormatEnum,
    render::BlendMode,
    surface::Surface,
    video::{FullscreenType, Window},
};

pub struct Output {
    pub window: Window,
    pub events: sdl2::EventPump,
    pub context: sdl2::Sdl,
    surface: Option<Surface<'static>>,
    close_message: String,
    close_caption: String,
}
impl Output {
    pub fn new(title: &str, size: [u32; 2]) -> Result<Self, Box<dyn std::error::Error>> {
        let context = sdl2::init()?;
        let video = context.video()?;
        let window = video
            .window(title, size[0], size[1])
            .position_centered()
            .build()?;
        let events = context.event_pump()?;
        Ok(Self {
            window,
            events,
            context,
            surface: None,
            close_message: "Quit the game?".to_owned(),
            close_caption: title.to_owned(),
        })
    }
    pub fn poll(&mut self) -> Vec<Event> {
        self.events.poll_iter().collect()
    }
    pub fn toggle_fullscreen(&mut self) -> Result<bool, String> {
        let fullscreen = self.window.fullscreen_state() == FullscreenType::Off;
        self.window.set_fullscreen(if fullscreen {
            FullscreenType::Desktop
        } else {
            FullscreenType::Off
        })?;
        Ok(self.window.fullscreen_state() != FullscreenType::Off)
    }
    pub fn configure_close(&mut self, tables: &katze_system2::language::StringTables, title: &str) {
        let template: Vec<_> = "$0000-0042$Quit %s?".encode_utf16().collect();
        self.close_message =
            String::from_utf16_lossy(tables.resolve(&template)).replacen("%s", title, 1);
        let caption: Vec<_> = "$0000-0043$Exit".encode_utf16().collect();
        self.close_caption = String::from_utf16_lossy(tables.resolve(&caption));
    }
    pub fn confirm_close(&self) -> bool {
        let buttons = [
            ButtonData {
                flags: MessageBoxButtonFlag::RETURNKEY_DEFAULT,
                button_id: 1,
                text: "Yes",
            },
            ButtonData {
                flags: MessageBoxButtonFlag::ESCAPEKEY_DEFAULT,
                button_id: 0,
                text: "No",
            },
        ];
        match sdl2::messagebox::show_message_box(
            MessageBoxFlag::empty(),
            &buttons,
            &self.close_caption,
            &self.close_message,
            &self.window,
            None,
        ) {
            Ok(ClickedButton::CustomButton(button)) => button.button_id == 1,
            Ok(ClickedButton::CloseButton) => false,
            Err(error) => {
                eprintln!("关闭确认失败：{error}");
                false
            }
        }
    }
    pub fn present(&mut self, frame: &RenderTarget) -> Result<(), String> {
        let size = frame.size();
        if self
            .surface
            .as_ref()
            .is_none_or(|s| s.size() != (size[0], size[1]))
        {
            let mut surface = Surface::new(size[0], size[1], PixelFormatEnum::BGRA32)?;
            surface.set_blend_mode(BlendMode::None)?;
            self.surface = Some(surface);
        }
        let surface = self.surface.as_mut().unwrap();
        let pitch = surface.pitch() as usize;
        surface.with_lock_mut(|pixels| {
            for y in 0..size[1] {
                for x in 0..size[0] {
                    let at = y as usize * pitch + x as usize * 4;
                    pixels[at..at + 4]
                        .copy_from_slice(&frame.pixel(x, y).unwrap_or(0).to_le_bytes());
                }
            }
        });
        let mut destination = self.window.surface(&self.events)?;
        surface.blit_scaled(None, &mut destination, None)?;
        destination.update_window()
    }
}

impl WindowServices for Output {
    fn toggle_fullscreen(&mut self) -> katze_system2::Result<bool> {
        Output::toggle_fullscreen(self).map_err(|message| katze_system2::Error::new(0, message))
    }
    fn minimize(&mut self) -> katze_system2::Result<()> {
        self.window.minimize();
        Ok(())
    }
    fn confirm_close(&mut self) -> bool {
        Output::confirm_close(self)
    }
}
