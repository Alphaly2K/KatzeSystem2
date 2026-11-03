use super::scripts::BackgroundSource;
use crate::runtime::{
    ObjectFactory, Store,
    fes::{ObjectState, Objects},
    route_with_buffers,
};
use crate::task::{Command, CommandBuffers};
use crate::{Error, Result};

impl<F: ObjectFactory> Store<'_, F> {
    pub(super) fn advance_fes_background(
        &mut self,
        owner: u32,
        objects: &Objects,
        source: &Option<BackgroundSource>,
        image: &mut u32,
        phase: &mut usize,
    ) -> Result<bool> {
        let list = self.check_fes_image_list(owner, objects)?;
        let (source, mut rectangle) = match source {
            Some(BackgroundSource::Plane(handle)) => {
                let Some(ObjectState::Plane(plane)) =
                    objects.get(*handle).map(|object| &object.state)
                else {
                    return Err(Error::new(0, "FES 背景平面已失效"));
                };
                match *phase {
                    0 => {
                        self.fes_command(owner, Some(list), 296, [0, 0, 1], None)?;
                        *phase = 1;
                        return Ok(false);
                    }
                    1 => {
                        let mut bytes = image.to_le_bytes();
                        self.fes_command(owner, Some(list), 291, [0; 3], Some(&mut bytes))?;
                        *image = u32::from_le_bytes(bytes);
                        *phase = 2;
                        return Ok(false);
                    }
                    _ => (),
                }
                let [x, y, _, _] = plane.vram();
                let [x, y] = [x, y].map(crate::runtime::fes::plane::integer_position);
                let [width, height] = plane.base.size();
                (
                    *image,
                    Some(
                        [x, y, x.wrapping_add(width), y.wrapping_add(height)]
                            .map(i32::to_le_bytes)
                            .concat(),
                    ),
                )
            }
            Some(BackgroundSource::Image(image)) => (*image, None),
            None => (0, None),
        };
        route_with_buffers(
            self.reborrow(),
            owner,
            Command {
                target: list,
                sender: owner,
                code: 302,
                data: [0, source, 1],
            },
            &mut CommandBuffers {
                data0: rectangle.as_deref_mut(),
                data1: None,
            },
        )?;
        Ok(true)
    }
}
