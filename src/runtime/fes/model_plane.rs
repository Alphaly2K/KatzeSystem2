use super::{ObjectState, Objects};
use crate::{
    Error, Result,
    fes::CPlaneState,
    graphics::Drawable,
    host::model::ModelSelection,
    runtime::{ObjectFactory, Store},
};

/// FES character model plane and its pending model selection.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelPlaneState {
    pub(super) plane: CPlaneState,
    selection: Vec<u8>,
    pending_selection: Option<Vec<u8>>,
    body_options: Vec<u8>,
    body_pending: bool,
    effects_mode: Option<u32>,
    applied_effects_mode: Option<u32>,
    pub(super) effects_active: bool,
}

impl ModelPlaneState {
    pub fn plane(&self) -> &CPlaneState {
        &self.plane
    }

    pub(super) fn set_effects_mode(&mut self, mode: u32) {
        self.effects_mode = Some(mode);
    }

    pub(super) fn set_body_options(&mut self, options: Option<&[u8]>) -> Result<()> {
        if let Some(options) = options {
            let options = options.split(|&byte| byte == 0).next().unwrap_or_default();
            if options.len() >= 0x300 {
                return Err(Error::new(0, "FES 模型主体参数超过存储上限"));
            }
            if self.body_options.eq_ignore_ascii_case(options) {
                return Ok(());
            }
            self.body_options.clear();
            self.body_options.extend_from_slice(options);
        }
        self.body_pending = true;
        Ok(())
    }

    pub(super) fn select(&mut self, selection: &[u8]) -> Result<()> {
        let selection = selection
            .split(|&byte| byte == 0)
            .next()
            .unwrap_or_default();
        if selection.len() >= 0x300 {
            return Err(Error::new(0, "FES 模型选择参数超过存储上限"));
        }
        if self.selection.eq_ignore_ascii_case(selection) {
            self.pending_selection = None;
        } else if !self
            .pending_selection
            .as_ref()
            .is_some_and(|pending| pending.eq_ignore_ascii_case(selection))
        {
            self.pending_selection = Some(selection.to_vec());
        }
        Ok(())
    }
}

impl<F: ObjectFactory> Store<'_, F> {
    pub(super) fn update_fes_model_plane(
        &mut self,
        objects: &mut Objects,
        handle: u32,
    ) -> Result<()> {
        let position = objects.plane_position(handle);
        let base = objects.plane_base_point(handle);
        let ObjectState::ModelPlane(state) = &mut objects.get_mut(handle).unwrap().state else {
            unreachable!()
        };
        let drawables = self.resources.compositor.drawables_mut();
        if let Some(selection) = state.pending_selection.as_deref() {
            let reused = match (
                ModelSelection::parse(&state.selection),
                ModelSelection::parse(selection),
                self.resources.model_players.get_mut(&handle),
            ) {
                (Some(current), Some(next), Some(player))
                    if current.profile.eq_ignore_ascii_case(next.profile)
                        && current.body.eq_ignore_ascii_case(next.body)
                        && current.motion.eq_ignore_ascii_case(next.motion) =>
                {
                    player.set_options(next.options)?
                }
                _ => false,
            };
            if !reused {
                let player = if selection.is_empty() {
                    None
                } else if let Some(services) = self.resources.model_services.as_mut() {
                    if let Some(resources) = self.resources.services.as_deref_mut() {
                        services.open_with_resources(selection, resources)?
                    } else {
                        services.open(selection)?
                    }
                } else {
                    None
                };
                if let Some(player) = player {
                    self.resources.model_players.insert(handle, player);
                    state.applied_effects_mode = Some(1);
                } else {
                    self.resources.model_players.remove(&handle);
                    drawables.remove(handle);
                }
            }
            state.selection = state.pending_selection.take().unwrap();
        }
        let Some(player) = self.resources.model_players.get_mut(&handle) else {
            state.body_pending = false;
            state.effects_active = false;
            return Ok(());
        };
        if state.body_pending {
            player.set_body_options(&state.body_options)?;
            state.body_pending = false;
        }
        if let Some(mode) = state
            .effects_mode
            .filter(|&mode| Some(mode) != state.applied_effects_mode)
        {
            player.set_effects_mode(mode)?;
            state.applied_effects_mode = Some(mode);
        }
        state.effects_active = player.effects_active(self.host.system_time_ms)?;
        if state.plane.display == 0 {
            drawables.set_display(handle, 0);
            return Ok(());
        }
        let Some(frame) = player.frame(self.host.system_time_ms)? else {
            drawables.set_display(handle, 0);
            return Ok(());
        };
        let size = frame.pixels.size().map(|axis| axis as f32);
        if drawables
            .get(handle)
            .is_none_or(|d| d.bounds()[1][..2] != size)
        {
            drawables.remove(handle);
            drawables.insert(handle, Drawable::image_rectangle([0.; 3], size))?;
        }
        drawables.set_image(handle, Some(frame.pixels));
        drawables.set_base_point(
            handle,
            [
                base[0] - frame.origin[0] as f32,
                base[1] - frame.origin[1] as f32,
                state.plane.priority as f32,
            ],
        );
        drawables.set_position(handle, [position[0], position[1], position[2]]);
        drawables.set_scale(handle, state.plane.scale()[..3].try_into().unwrap());
        drawables.set_matrix(handle, *state.plane.matrix());
        drawables.set_color(handle, state.plane.color, 1);
        drawables.set_display(handle, state.plane.display as u32);
        state.plane.dirty = false;
        Ok(())
    }
}
