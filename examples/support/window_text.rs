use katze_system2::{
    graphics::{Rect, Surface},
    input::InputState,
    message::{FontRequest, FontServices, Fonts, TextEncoding},
};
use sdl2::{
    event::{Event, WindowEvent},
    keyboard::{Mod, Scancode},
    mouse::MouseButton,
    pixels::PixelFormatEnum,
    render::Canvas,
    video::Window,
};
use std::{ops::Range, sync::Arc};

const SIZE: [u32; 2] = [460, 116];
const FIELD: Rect = Rect {
    left: 16,
    top: 16,
    right: 444,
    bottom: 50,
};
const OK: Rect = Rect {
    left: 258,
    top: 66,
    right: 344,
    bottom: 100,
};
const CANCEL: Rect = Rect {
    left: 358,
    top: 66,
    right: 444,
    bottom: 100,
};
const FONT_SIZE: u32 = 18;
const MAX_UNITS: usize = 16;

struct Edit {
    text: String,
    caret: usize,
    anchor: usize,
    marked: String,
}

impl Edit {
    fn new(initial: &str) -> Self {
        Self {
            text: initial.to_owned(),
            caret: initial.len(),
            anchor: 0,
            marked: String::new(),
        }
    }

    fn selection(&self) -> Range<usize> {
        self.anchor.min(self.caret)..self.anchor.max(self.caret)
    }

    fn move_to(&mut self, at: usize, extend: bool) {
        self.caret = at;
        if !extend {
            self.anchor = at;
        }
    }

    fn neighbor(&self, forward: bool) -> usize {
        if forward {
            self.caret
                + self.text[self.caret..]
                    .chars()
                    .next()
                    .map_or(0, char::len_utf8)
        } else {
            self.text[..self.caret]
                .char_indices()
                .next_back()
                .map_or(0, |(i, _)| i)
        }
    }

    fn insert(&mut self, text: &str) {
        let range = self.selection();
        let retained = self.text[..range.start].encode_utf16().count()
            + self.text[range.end..].encode_utf16().count();
        let mut room = MAX_UNITS.saturating_sub(retained);
        let insert: String = text
            .chars()
            .filter(|c| !c.is_control())
            .take_while(|c| {
                let fits = c.len_utf16() <= room;
                room = room.saturating_sub(c.len_utf16());
                fits
            })
            .collect();
        if !insert.is_empty() {
            let end = range.start + insert.len();
            self.text.replace_range(range, &insert);
            self.move_to(end, false);
        }
    }

    fn delete(&mut self, forward: bool) {
        if self.selection().is_empty() {
            self.anchor = self.neighbor(forward);
        }
        let range = self.selection();
        self.move_to(range.start, false);
        self.text.replace_range(range, "");
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Confirm,
    Cancel,
}

struct Dialog {
    canvas: Canvas<Window>,
    parent: u32,
    request: Arc<str>,
    edit: Edit,
    dirty: bool,
    stops: Vec<(i32, usize)>,
}

impl Dialog {
    fn new(parent: &Window, request: Arc<str>) -> Result<Self, Box<dyn std::error::Error>> {
        let (x, y) = parent.position();
        let (w, h) = parent.size();
        let canvas = parent
            .subsystem()
            .window("Text Input", SIZE[0], SIZE[1])
            .position(
                x + (w as i32 - SIZE[0] as i32) / 2,
                y + (h as i32 - SIZE[1] as i32) / 2,
            )
            .build()?
            .into_canvas()
            .software()
            .build()?;
        let edit = Edit::new(&request);
        let mut dialog = Self {
            canvas,
            parent: parent.id(),
            request,
            edit,
            dirty: true,
            stops: Vec::new(),
        };
        dialog.canvas.window_mut().raise();
        let input = dialog.canvas.window().subsystem().text_input();
        input.start();
        Ok(dialog)
    }

    fn event(&mut self, event: &Event) -> Option<Action> {
        let id = self.canvas.window().id();
        match event {
            Event::Window {
                window_id,
                win_event: WindowEvent::FocusGained,
                ..
            } if *window_id == self.parent => self.canvas.window_mut().raise(),
            Event::Window {
                window_id,
                win_event,
                ..
            } if *window_id == id => match win_event {
                WindowEvent::Close => return Some(Action::Cancel),
                WindowEvent::Exposed | WindowEvent::Shown => self.dirty = true,
                WindowEvent::FocusLost => {
                    self.edit.marked.clear();
                    self.dirty = true;
                }
                _ => {}
            },
            Event::TextEditing {
                window_id, text, ..
            } if *window_id == id => {
                self.edit.marked.clone_from(text);
                self.dirty = true;
            }
            Event::TextInput {
                window_id, text, ..
            } if *window_id == id => {
                self.edit.marked.clear();
                self.edit.insert(text);
                self.dirty = true;
            }
            Event::KeyDown {
                window_id,
                scancode: Some(key),
                keymod,
                repeat,
                ..
            } if *window_id == id && self.edit.marked.is_empty() => {
                let shift = keymod.intersects(Mod::LSHIFTMOD | Mod::RSHIFTMOD);
                let shortcut =
                    keymod.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD | Mod::LGUIMOD | Mod::RGUIMOD);
                match key {
                    Scancode::Return | Scancode::KpEnter if !repeat => {
                        return Some(Action::Confirm);
                    }
                    Scancode::Escape if !repeat => return Some(Action::Cancel),
                    Scancode::Backspace => self.edit.delete(false),
                    Scancode::Delete => self.edit.delete(true),
                    Scancode::Left | Scancode::Right => {
                        let forward = *key == Scancode::Right;
                        let selected = self.edit.selection();
                        let at = if !shift && !selected.is_empty() {
                            if forward {
                                selected.end
                            } else {
                                selected.start
                            }
                        } else {
                            self.edit.neighbor(forward)
                        };
                        self.edit.move_to(at, shift);
                    }
                    Scancode::Home => self.edit.move_to(0, shift),
                    Scancode::End => self.edit.move_to(self.edit.text.len(), shift),
                    Scancode::A if shortcut => {
                        self.edit.anchor = 0;
                        self.edit.caret = self.edit.text.len();
                    }
                    Scancode::C | Scancode::X if shortcut => {
                        let selection = self.edit.selection();
                        if !selection.is_empty()
                            && self
                                .canvas
                                .window()
                                .subsystem()
                                .clipboard()
                                .set_clipboard_text(&self.edit.text[selection])
                                .is_ok()
                            && *key == Scancode::X
                        {
                            self.edit.delete(false);
                        }
                    }
                    Scancode::V if shortcut => {
                        if let Ok(text) = self
                            .canvas
                            .window()
                            .subsystem()
                            .clipboard()
                            .clipboard_text()
                        {
                            self.edit.insert(&text);
                        }
                    }
                    _ => return None,
                }
                self.dirty = true;
            }
            Event::MouseButtonDown {
                window_id,
                mouse_btn: MouseButton::Left,
                x,
                y,
                ..
            } if *window_id == id && self.edit.marked.is_empty() => {
                let contains = |rect: Rect| {
                    (*x >= rect.left && *x < rect.right) && (*y >= rect.top && *y < rect.bottom)
                };
                if contains(OK) {
                    return Some(Action::Confirm);
                }
                if contains(CANCEL) {
                    return Some(Action::Cancel);
                }
                if !self.dirty && contains(FIELD) {
                    if let Some(&(_, at)) = self.stops.iter().min_by_key(|(at, _)| x.abs_diff(*at))
                    {
                        self.edit.move_to(at, false);
                        self.dirty = true;
                    }
                }
            }
            _ => {}
        }
        None
    }

    fn draw(&mut self, fonts: &mut Fonts) -> Result<(), Box<dyn std::error::Error>> {
        if !self.dirty {
            return Ok(());
        }
        let request = FontRequest {
            encoding: TextEncoding::Unicode,
            name: Vec::new(),
            weight: 400,
            italic: false,
            backend: 0,
            glyph_parameter: 0,
            measure_parameter: 0,
        };
        let mut frame = Surface::new(SIZE)?;
        frame.clear(0xffeeefef);
        frame.fill_rect(FIELD, 0xff4a6677);
        let mut field = Surface::new([426, 32])?;
        field.clear(0xffffffff);
        let selected = self.edit.selection();
        let left = &self.edit.text[..if self.edit.marked.is_empty() {
            self.edit.caret
        } else {
            selected.start
        }];
        let right_at = if self.edit.marked.is_empty() {
            self.edit.caret
        } else {
            selected.end
        };
        let mut glyphs = Vec::new();
        let mut x = 0;
        let mut caret_x = 0;
        for (origin, text, marked) in [
            (0, left, false),
            (left.len(), self.edit.marked.as_str(), true),
            (right_at, &self.edit.text[right_at..], false),
        ] {
            for (index, ch) in text.char_indices() {
                let mask = fonts.rasterize(&request, ch, FONT_SIZE)?;
                let advance = mask.advance().max(0);
                glyphs.push((x, advance, origin + index, marked, mask));
                x += advance;
            }
            if marked {
                caret_x = x;
            }
        }
        let scroll = (caret_x - 412).max(0);
        let offset = 6 - scroll;
        self.stops.clear();
        for (x, advance, index, marked, mask) in glyphs {
            let x = x + offset;
            if marked {
                field.fill_rect(Rect::from_xywh(x, 26, advance, 1), 0xff23789f);
            } else {
                self.stops.push((FIELD.left + 1 + x, index));
                if selected.contains(&index) && self.edit.marked.is_empty() {
                    field.fill_rect(Rect::from_xywh(x, 4, advance, 24), 0xffcce5f1);
                }
            }
            let glyph = mask.colorize(0xff232323, false)?;
            field.blend_from(
                &glyph,
                glyph.bounds(),
                [x + mask.offset()[0], 7 + mask.offset()[1]],
                255,
            );
        }
        self.stops
            .push((FIELD.left + 1 + x + offset, self.edit.text.len()));
        let caret_x = caret_x + offset;
        field.fill_rect(Rect::from_xywh(caret_x, 4, 1, 24), 0xff244c61);
        frame.copy_from(&field, field.bounds(), [FIELD.left + 1, FIELD.top + 1]);
        self.canvas
            .window()
            .subsystem()
            .text_input()
            .set_rect(sdl2::rect::Rect::new(
                FIELD.left + 1 + caret_x,
                FIELD.top,
                1,
                34,
            ));
        for (rect, label, fill, color) in [
            (OK, "OK", 0xff275b72, 0xffffffff),
            (CANCEL, "Cancel", 0xffdde0e2, 0xff232323),
        ] {
            frame.fill_rect(rect, fill);
            let glyphs = label
                .chars()
                .map(|ch| fonts.rasterize(&request, ch, FONT_SIZE))
                .collect::<katze_system2::Result<Vec<_>>>()?;
            let width: i32 = glyphs.iter().map(|mask| mask.advance()).sum();
            let mut x = (rect.left + rect.right - width) / 2;
            for mask in glyphs {
                let glyph = mask.colorize(color, false)?;
                frame.blend_from(
                    &glyph,
                    glyph.bounds(),
                    [x + mask.offset()[0], rect.top + 8 + mask.offset()[1]],
                    255,
                );
                x += mask.advance();
            }
        }
        let creator = self.canvas.texture_creator();
        let mut texture =
            creator.create_texture_streaming(PixelFormatEnum::BGRA32, SIZE[0], SIZE[1])?;
        texture.update(None, frame.pixels(), (SIZE[0] * 4) as usize)?;
        self.canvas.copy(&texture, None, None)?;
        self.canvas.present();
        self.dirty = false;
        Ok(())
    }
}

impl Drop for Dialog {
    fn drop(&mut self) {
        self.canvas.window().subsystem().text_input().stop();
    }
}

#[derive(Default)]
pub struct TextWindow {
    dialog: Option<Dialog>,
    fonts: Option<Fonts>,
}

impl TextWindow {
    pub fn active(&self) -> bool {
        self.dialog.is_some()
    }

    pub fn event(
        &mut self,
        event: &Event,
        input: Option<&mut InputState>,
    ) -> katze_system2::Result<()> {
        if let Some(dialog) = &mut self.dialog {
            if let Some(action) = dialog.event(event) {
                if let Some(input) = input {
                    let text = (action == Action::Confirm).then_some(dialog.edit.text.as_str());
                    input.complete_text_entry(&dialog.request, text)?;
                }
                self.dialog = None;
            }
        }
        Ok(())
    }

    pub fn sync(
        &mut self,
        parent: &Window,
        input: Option<&InputState>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let request = input.and_then(InputState::text_entry_request);
        if request.as_ref().is_some_and(|request| {
            self.dialog
                .as_ref()
                .is_some_and(|dialog| Arc::ptr_eq(request, &dialog.request))
        }) {
            return Ok(());
        }
        self.dialog = None;
        if let Some(request) = request {
            self.dialog = Some(Dialog::new(parent, request)?);
        }
        Ok(())
    }

    pub fn draw(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(dialog) = &mut self.dialog {
            dialog.draw(self.fonts.get_or_insert_with(|| Fonts::new(".")))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Edit, MAX_UNITS};

    #[test]
    fn editing_replaces_selection_and_never_splits_a_utf16_pair() {
        let initial = "a".repeat(MAX_UNITS + 1);
        let mut edit = Edit::new(&initial);
        assert_eq!(edit.text, initial);
        edit.insert("abcdefghijklmn\u{1f600}z");
        assert_eq!(edit.text, "abcdefghijklmn\u{1f600}");
        assert_eq!(edit.neighbor(false), MAX_UNITS - 2);
        edit.delete(false);
        edit.insert("\t\u{65e5}\u{672c}\0z");
        assert_eq!(edit.text, "abcdefghijklmn\u{65e5}\u{672c}");
        edit.move_to(edit.neighbor(false), true);
        edit.insert("\u{1f600}");
        assert!(!edit.selection().is_empty());
        edit.delete(true);
        assert_eq!(edit.text, "abcdefghijklmn\u{65e5}");
    }
}
