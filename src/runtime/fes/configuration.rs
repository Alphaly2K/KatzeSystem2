use super::{ObjectState, Objects};
use crate::Result;
use crate::runtime::{ObjectFactory, Store};

impl<F: ObjectFactory> Store<'_, F> {
    pub(super) fn refresh_fes_configuration_record(
        &mut self,
        owner: u32,
        objects: &mut Objects,
        cursor: usize,
    ) -> Result<()> {
        let record = &objects.update_order[cursor];
        let Some(handle) = objects.media_handle(record) else {
            return Ok(());
        };
        match record.kind {
            "STRING" | "STRING8" => {
                if let ObjectState::String(state) = &mut objects.get_mut(handle).unwrap().state {
                    state.layout.refresh_font_defaults(
                        self.host
                            .message_defaults
                            .as_ref()
                            .unwrap_or(&Default::default()),
                    )?;
                }
            }
            "SOUND" | "SNDTABLE" => self.refresh_fes_sound_configuration(owner, objects, handle)?,
            _ => (),
        }
        Ok(())
    }
}
