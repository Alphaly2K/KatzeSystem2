use super::{ObjectState, Objects};
use crate::Result;
use crate::runtime::{ObjectFactory, Store};

impl<F: ObjectFactory> Store<'_, F> {
    pub(super) fn update_fes_viewport(&mut self, objects: &mut Objects, handle: u32) -> Result<()> {
        let ObjectState::Viewport(state) = &mut objects.get_mut(handle).unwrap().state else {
            unreachable!()
        };
        state.dirty = false;
        if !state.rebuild {
            return Ok(());
        }
        let resource = state.size_resource.clone();
        let cache = objects.preloaded_images.clone();
        if let Some(resource) = resource {
            let size = self.fes_image_geometry(&cache, &resource, true)?;
            let ObjectState::Viewport(state) = &mut objects.get_mut(handle).unwrap().state else {
                unreachable!()
            };
            if state.size != size {
                state.size = size;
                objects.invalidate(handle)?;
            }
        }
        let ObjectState::Viewport(state) = &mut objects.get_mut(handle).unwrap().state else {
            unreachable!()
        };
        if let Some(resource) = state.position_resource.take() {
            let position = self
                .fes_image_geometry(&cache, &resource, false)?
                .map(|v| v as f32);
            let ObjectState::Viewport(state) = &mut objects.get_mut(handle).unwrap().state else {
                unreachable!()
            };
            if state.position != position {
                state.position = position;
                objects.invalidate(handle)?;
            }
        }
        let ObjectState::Viewport(state) = &mut objects.get_mut(handle).unwrap().state else {
            unreachable!()
        };
        state.rebuild = false;
        Ok(())
    }
}
