use super::{Drawable, Drawables};
use crate::Result;
mod history;
use history::{History, Snapshot};

impl Snapshot {
    pub fn capture(drawable: &Drawable) -> Self {
        let transform = drawable.transform();
        Self {
            base: transform.base_point(),
            position: transform.position(),
            scale: transform.scale(),
            matrix: transform.rotation_matrix(),
            color: drawable.color().0,
            display: drawable.display(),
        }
    }

    fn apply(&self, id: u32, drawings: &mut Drawables) {
        drawings.set_base_point(id, self.base);
        drawings.set_position(id, self.position);
        drawings.set_scale(id, self.scale);
        drawings.set_matrix(id, self.matrix);
        drawings.set_color(id, self.color, 1);
        drawings.set_display(id, self.display);
    }
}

pub(super) struct Echo {
    state: History,
    active: bool,
}

impl Echo {
    pub fn new(bytes: &[u8], source: &Drawable, size: [f32; 2]) -> Result<Self> {
        Ok(Self {
            state: History::new(bytes, Snapshot::capture(source), size)?,
            active: false,
        })
    }
    pub fn restore(&mut self, bytes: &[u8]) -> Result<()> {
        if self.active {
            self.state.advance(Snapshot::read(bytes)?);
        }
        Ok(())
    }
    pub fn register(&mut self, id: u32, drawings: &mut Drawables) -> Result<()> {
        let Some(source) = drawings.get(id).cloned() else {
            return Ok(());
        };
        let Some(geometry) = source.geometry_transform(true).cloned() else {
            return Ok(());
        };
        for i in 1..self.state.history.len() {
            if self.state.created(i) {
                let copy_id = id.wrapping_shl(16) | i as u32;
                drawings.remove(copy_id);
                let mut copy = source.clone();
                copy.display = 0;
                if let Err(error) = drawings.insert(copy_id, copy) {
                    self.release(id, drawings);
                    return Err(error);
                }
            }
        }
        for (i, transform) in self.state.geometry_states(&geometry) {
            let selected = if i == 0 {
                id
            } else {
                id.wrapping_shl(16) | i as u32
            };
            drawings.update(selected, |drawable| {
                if let Some(source) = &mut drawable.canvas_source {
                    source.geometry.transform = transform;
                } else {
                    drawable.transform = transform;
                }
            });
        }
        self.active = true;
        Ok(())
    }

    pub fn update(&mut self, id: u32, drawings: &mut Drawables) {
        if !self.active {
            return;
        }
        let Some(source) = drawings.get(id) else {
            return;
        };
        self.state.advance(Snapshot::capture(source));
        for (i, state) in self.state.sample() {
            if i == 0 {
                drawings.set_matrix(id, state.matrix);
            } else {
                state.apply(id.wrapping_shl(16) | i as u32, drawings);
            }
        }
    }

    pub fn release(&mut self, id: u32, drawings: &mut Drawables) {
        for i in 1..self.state.history.len() {
            if self.state.created(i) {
                drawings.remove(id.wrapping_shl(16) | i as u32);
            }
        }
        self.active = false;
    }

    pub fn set_blend_mode(&self, id: u32, value: u32, drawings: &mut Drawables) {
        for i in 1..self.state.history.len() {
            if self.state.created(i) {
                drawings.set_blend_mode(id.wrapping_shl(16) | i as u32, value);
            }
        }
    }

    pub fn set_display(&self, id: u32, value: u32, drawings: &mut Drawables) {
        for i in 1..self.state.history.len() {
            if self.state.sampled(i) {
                drawings.set_display(id.wrapping_shl(16) | i as u32, value);
            }
        }
    }
}
