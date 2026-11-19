use katze_system2::{
    graphics::{Rect, Surface},
    host::{QuizDebugInput, QuizDebugSession},
    message::{FontRequest, FontServices, Fonts, TextEncoding},
    session::GameRuntime,
};
use sdl2::{
    event::{Event, WindowEvent},
    keyboard::Scancode,
    mouse::{MouseButton, MouseWheelDirection},
    pixels::PixelFormatEnum,
    render::Canvas,
    video::Window,
};
use std::collections::{BTreeMap, btree_map::Entry};

const SIZE: [u32; 2] = [500, 326];
const ROWS: usize = 9;
const ROW_HEIGHT: i32 = 22;
const LIST_TOP: i32 = 88;
const LIST_W: i32 = 228;
const LIST_X: [i32; 2] = [16, 256];

#[derive(Default)]
struct Scroll {
    top: usize,
    selected: Option<usize>,
}

impl Scroll {
    fn sync(&mut self, selected: Option<usize>, len: usize) {
        if self.selected != selected {
            self.selected = selected;
            if let Some(selected) = selected {
                self.top = self
                    .top
                    .clamp((selected + 1).saturating_sub(ROWS), selected);
            }
        }
        self.top = self.top.min(len.saturating_sub(ROWS));
    }

    fn shift(&mut self, by: isize, len: usize) {
        self.top = self
            .top
            .saturating_add_signed(by)
            .min(len.saturating_sub(ROWS));
    }

    fn hit(&self, y: i32, len: usize) -> Option<usize> {
        if !(LIST_TOP..LIST_TOP + ROW_HEIGHT * ROWS as i32).contains(&y) {
            return None;
        }
        let row = ((y - LIST_TOP) / ROW_HEIGHT) as usize;
        let index = self.top + row;
        (index < len).then_some(index)
    }
}

#[derive(PartialEq, Eq)]
struct View {
    checks: [bool; 4],
    lists: [Vec<(String, bool)>; 2],
    pages: [(usize, usize); 2],
}

struct Panel {
    canvas: Canvas<Window>,
    scroll: [Scroll; 2],
    last: Option<View>,
}

impl Panel {
    fn new(parent: &Window, task: u32) -> Result<Self, Box<dyn std::error::Error>> {
        let canvas = parent
            .subsystem()
            .window(&format!("Quiz Debug - {task}"), SIZE[0], SIZE[1])
            .position_centered()
            .build()?
            .into_canvas()
            .software()
            .build()?;
        Ok(Self {
            canvas,
            scroll: Default::default(),
            last: None,
        })
    }

    fn event(&mut self, event: &Event, session: &QuizDebugSession) -> Option<QuizDebugInput> {
        let lens = [session.packs.len(), session.questions.len()];
        let column = |x| {
            LIST_X
                .iter()
                .position(|&left| x >= left && x < left + LIST_W)
        };
        match *event {
            Event::Window {
                win_event: WindowEvent::Close,
                ..
            }
            | Event::KeyDown {
                scancode: Some(Scancode::Escape),
                repeat: false,
                ..
            } => Some(QuizDebugInput::Close),
            Event::Window {
                win_event: WindowEvent::Exposed | WindowEvent::Shown,
                ..
            } => {
                self.last = None;
                None
            }
            Event::MouseButtonDown {
                mouse_btn: MouseButton::Left,
                x,
                y,
                ..
            } => {
                let col = column(x)?;
                if (14..36).contains(&y) && col == 0 {
                    return Some(QuizDebugInput::Enable(!session.enabled));
                }
                if !session.enabled {
                    return None;
                }
                match (col, y) {
                    (1, 14..=35) => Some(QuizDebugInput::LockQuestionAndAnswers(
                        !session.lock_question_and_answers,
                    )),
                    (0, 38..=59) => Some(QuizDebugInput::PauseCounting(!session.pause_counting)),
                    (1, 38..=59) => Some(QuizDebugInput::AutoAdvance(!session.auto_advance)),
                    (_, 288..=311) => {
                        let left = LIST_X[col];
                        if x < left + 32 {
                            self.scroll[col].shift(-(ROWS as isize), lens[col]);
                        } else if x >= left + LIST_W - 32 {
                            self.scroll[col].shift(ROWS as isize, lens[col]);
                        }
                        None
                    }
                    _ => self.scroll[col].hit(y, lens[col]).map(|index| {
                        if col == 0 {
                            self.scroll[1] = Scroll::default();
                            QuizDebugInput::SelectPack(index)
                        } else {
                            QuizDebugInput::SelectQuestion(index)
                        }
                    }),
                }
            }
            Event::MouseWheel {
                y,
                direction,
                mouse_x,
                mouse_y,
                ..
            } if session.enabled
                && (LIST_TOP..LIST_TOP + ROW_HEIGHT * ROWS as i32).contains(&mouse_y) =>
            {
                let col = column(mouse_x)?;
                let y = y as isize;
                self.scroll[col].shift(
                    if direction == MouseWheelDirection::Flipped {
                        y
                    } else {
                        -y
                    },
                    lens[col],
                );
                None
            }
            _ => None,
        }
    }

    fn draw(
        &mut self,
        session: &QuizDebugSession,
        encoding: &'static encoding_rs::Encoding,
        fonts: &mut Fonts,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let lens = [session.packs.len(), session.questions.len()];
        self.scroll[0].sync(session.pack, lens[0]);
        self.scroll[1].sync(session.question, lens[1]);
        let checks = [
            session.enabled,
            session.lock_question_and_answers,
            session.pause_counting,
            session.auto_advance,
        ];
        let lists = std::array::from_fn(|col| {
            let list = &self.scroll[col];
            (list.top..(list.top + ROWS).min(lens[col]))
                .map(|index| {
                    let label = if col == 0 {
                        encoding
                            .decode_without_bom_handling(&session.packs[index])
                            .0
                            .chars()
                            .filter(|c| !c.is_control())
                            .take(128)
                            .collect()
                    } else {
                        (session.questions[index] as i32).to_string()
                    };
                    (label, list.selected == Some(index))
                })
                .collect()
        });
        let pages = std::array::from_fn(|col| (self.scroll[col].top, lens[col]));
        let view = View {
            checks,
            lists,
            pages,
        };
        if self.last.as_ref() == Some(&view) {
            return Ok(());
        }

        let mut frame = Surface::new(SIZE)?;
        frame.clear(0xffeeefef);
        for (i, label) in [
            "Enable",
            "Lock question and answers",
            "Pause counting",
            "Auto advance",
        ]
        .into_iter()
        .enumerate()
        {
            let x = LIST_X[i % 2];
            let y = 14 + (i / 2) as i32 * 24;
            let active = i == 0 || checks[0];
            frame.fill_rect(Rect::from_xywh(x, y + 3, 16, 16), 0xff66777e);
            frame.fill_rect(Rect::from_xywh(x + 1, y + 4, 14, 14), 0xffffffff);
            if checks[i] {
                frame.fill_rect(
                    Rect::from_xywh(x + 4, y + 7, 8, 8),
                    if active { 0xff275b72 } else { 0xff798387 },
                );
            }
            label_at(
                &mut frame,
                fonts,
                [x + 22, y],
                LIST_W - 22,
                label,
                if active { 0xff232323 } else { 0xff636f74 },
            )?;
        }
        for (col, heading) in ["Question packs", "Question IDs"].into_iter().enumerate() {
            let x = LIST_X[col];
            label_at(&mut frame, fonts, [x, 64], LIST_W, heading, 0xff232323)?;
            frame.fill_rect(
                Rect::from_xywh(x, LIST_TOP, LIST_W, ROW_HEIGHT * ROWS as i32),
                if checks[0] { 0xffffffff } else { 0xffe2e5e6 },
            );
            for (row, (text, selected)) in view.lists[col].iter().enumerate() {
                let y = LIST_TOP + row as i32 * ROW_HEIGHT;
                let highlighted = checks[0] && *selected;
                if highlighted {
                    frame.fill_rect(Rect::from_xywh(x, y, LIST_W, ROW_HEIGHT), 0xff275b72);
                }
                label_at(
                    &mut frame,
                    fonts,
                    [x + 6, y],
                    LIST_W - 12,
                    text,
                    if highlighted {
                        0xffffffff
                    } else if checks[0] {
                        0xff232323
                    } else {
                        0xff636f74
                    },
                )?;
            }
            let (top, count) = view.pages[col];
            let pages = if count == 0 {
                "0".to_owned()
            } else {
                format!("{}-{} / {count}", top + 1, (top + ROWS).min(count))
            };
            for (left, text, enabled) in [
                (x, "<", top != 0),
                (x + LIST_W - 32, ">", top + ROWS < count),
            ] {
                frame.fill_rect(Rect::from_xywh(left, 288, 32, 24), 0xffdce1e3);
                label_at(
                    &mut frame,
                    fonts,
                    [left + 10, 288],
                    20,
                    text,
                    if checks[0] && enabled {
                        0xff232323
                    } else {
                        0xff879296
                    },
                )?;
            }
            label_at(
                &mut frame,
                fonts,
                [x + 48, 288],
                LIST_W - 96,
                &pages,
                0xff39464b,
            )?;
        }
        let creator = self.canvas.texture_creator();
        let mut texture =
            creator.create_texture_streaming(PixelFormatEnum::BGRA32, SIZE[0], SIZE[1])?;
        texture.update(None, frame.pixels(), SIZE[0] as usize * 4)?;
        self.canvas.copy(&texture, None, None)?;
        self.canvas.present();
        self.last = Some(view);
        Ok(())
    }
}

fn label_at(
    frame: &mut Surface,
    fonts: &mut Fonts,
    [left, y]: [i32; 2],
    width: i32,
    text: &str,
    color: u32,
) -> katze_system2::Result<()> {
    let request = FontRequest {
        encoding: TextEncoding::Unicode,
        name: Vec::new(),
        weight: 400,
        italic: false,
        backend: 0,
        glyph_parameter: 0,
        measure_parameter: 0,
    };
    let mut x = left;
    let right = left + width;
    for ch in text.chars() {
        let mask = fonts.rasterize(&request, ch, 15)?;
        let glyph = mask.colorize(color, false)?;
        let advance = mask.advance().max(0);
        let gx = x + mask.offset()[0];
        if x + advance > right || gx + glyph.size()[0] as i32 > right {
            break;
        }
        frame.blend_from(&glyph, glyph.bounds(), [gx, y + 4 + mask.offset()[1]], 255);
        x += advance;
    }
    Ok(())
}

#[derive(Default)]
pub struct QuizWindows {
    panels: BTreeMap<u32, Panel>,
    fonts: Option<Fonts>,
}

impl QuizWindows {
    pub fn event(
        &mut self,
        event: &Event,
        runtime: &mut GameRuntime,
    ) -> katze_system2::Result<bool> {
        let Some(window) = event.get_window_id() else {
            return Ok(false);
        };
        let Some((&id, panel)) = self
            .panels
            .iter_mut()
            .find(|(_, panel)| panel.canvas.window().id() == window)
        else {
            return Ok(false);
        };
        if let Some(session) = runtime
            .quiz_debug_session(id)
            .filter(|session| session.visible)
        {
            if let Some(action) = panel.event(event, session) {
                runtime.quiz_debug_input(id, action)?;
            }
        }
        Ok(true)
    }

    pub fn sync(
        &mut self,
        parent: &Window,
        runtime: &GameRuntime,
        encoding: &'static encoding_rs::Encoding,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.panels.retain(|&id, _| {
            runtime
                .quiz_debug_session(id)
                .is_some_and(|session| session.visible)
        });
        for (id, session) in runtime.visible_quiz_debug_sessions() {
            let panel = match self.panels.entry(id) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => entry.insert(Panel::new(parent, id)?),
            };
            panel.draw(
                session,
                encoding,
                self.fonts
                    .get_or_insert_with(|| crate::support::load_fonts(".")),
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiz_selection_uses_visible_rows_and_follows_script_changes() {
        let mut scroll = Scroll::default();
        scroll.shift(9, 25);
        assert_eq!(scroll.hit(LIST_TOP + ROW_HEIGHT * 2, 25), Some(11));
        scroll.sync(Some(24), 25);
        assert_eq!(scroll.hit(LIST_TOP + ROW_HEIGHT * 8, 25), Some(24));
        scroll.sync(Some(0), 2);
        assert_eq!(scroll.hit(LIST_TOP, 2), Some(0));
        assert_eq!(scroll.hit(LIST_TOP - 1, 2), None);
        assert_eq!(scroll.hit(LIST_TOP + ROW_HEIGHT * 2, 2), None);
    }
}
