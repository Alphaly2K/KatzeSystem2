use super::{Object, ObjectState, Objects};
use crate::{Error, Result, fes};

/// 导航可选中目标、由关联控件消费，或回传默认输入响应。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NavigationOutcome {
    Target(fes::ObjectReference),
    Handled,
    Unhandled,
}

impl Objects {
    pub(super) fn enabled_button(&self, target: &fes::ObjectReference) -> bool {
        self.lookup(&target.name, target.bank)
            .and_then(|handle| self.get(handle))
            .is_some_and(enabled)
    }

    pub(super) fn navigation_hit(
        &self,
        point: [i32; 2],
        host: &dyn crate::kcs::Host,
    ) -> Result<Option<fes::ObjectReference>> {
        for record in self.media_records() {
            let Some(handle) = self.lookup(&record.target.name, record.target.bank) else {
                continue;
            };
            let object = self.get(handle).unwrap();
            let ObjectState::Button(button) = &object.state else {
                continue;
            };
            let origin = self.navigation_position(handle);
            let end: [i32; 2] =
                std::array::from_fn(|i| origin[i].wrapping_add(button.image.base.size[i]));
            if !(0..2).all(|i| origin[i] <= point[i] && point[i] < end[i]) {
                continue;
            }
            if button.mask_type != 0 {
                if let Some(wrapper) = object.image {
                    let image = host.image(wrapper)?;
                    let pixel: [u32; 2] = std::array::from_fn(|i| {
                        point[i]
                            .wrapping_sub(origin[i])
                            .wrapping_sub(image.info().offset[i]) as u32
                    });
                    if image.surface().pixel(pixel[0], pixel[1]).unwrap_or(0) >> 24 == 0 {
                        continue;
                    }
                }
            }
            return Ok(Some(record.target.clone()));
        }
        Ok(None)
    }

    pub(super) fn navigation_pointer(&self, target: &fes::ObjectReference) -> Option<[i32; 2]> {
        let handle = self.lookup(&target.name, target.bank)?;
        let state = self.get(handle)?.state.image()?;
        let mut point = self.navigation_position(handle);
        for (axis, offset) in point.iter_mut().zip(state.key_position) {
            *axis = axis.wrapping_add(offset);
        }
        if let Some(plane) = self.lookup(&state.plane.name, state.plane.bank) {
            if self
                .get(plane)
                .unwrap()
                .state
                .plane()
                .is_some_and(|state| state.extended_position() != 0)
            {
                for (axis, offset) in point.iter_mut().zip(self.plane_position(plane)) {
                    *axis = axis.wrapping_add(integer_position(offset));
                }
            }
        }
        Some(point)
    }

    /// 在 KEYBLOCK 中查找方向目标；方向顺序为上、下、左、右。
    /// 仅遍历当前名称绑定；KEY_* 属性不参与此版本的导航选择。
    pub fn navigate_button(
        &self,
        blocks: &[fes::KeyBlock],
        source: &fes::ObjectReference,
        direction: usize,
    ) -> Result<Option<fes::ObjectReference>> {
        let (dx, dy) = match direction {
            0 => (0, -1),
            1 => (0, 1),
            2 => (-1, 0),
            3 => (1, 0),
            _ => return Err(Error::new(0, "FES 导航方向超出 0～3")),
        };
        let Some(handle) = self.lookup(&source.name, source.bank) else {
            return Ok(None);
        };
        let ObjectState::Button(button) = &self.get(handle).unwrap().state else {
            return Ok(None);
        };
        if button.enabled == 0 || button.keyblock[0] < 0 {
            return Ok(None);
        }
        let [mut block, mut x, mut y] = button.keyblock;
        x = x.wrapping_add(dx);
        y = y.wrapping_add(dy);
        let mut origin = handle;
        let mut remaining = crate::read::MAX_ITEMS;
        loop {
            consume(&mut remaining)?;
            let grid = usize::try_from(block)
                .ok()
                .and_then(|i| blocks.get(i))
                .ok_or_else(|| Error::new(0, "FES 对象的 KEYBLOCK 编号无效"))?;
            while match direction {
                0 => y >= 0,
                1 => y < grid.size[1],
                2 => x >= 0,
                3 => x < grid.size[0],
                _ => unreachable!(),
            } {
                let mut found = self
                    .navigation_cell(&mut remaining, block, x, y)?
                    .filter(|object| enabled(object));
                if found.is_none() {
                    let center = if direction < 2 { x } else { y };
                    let limit = grid.size[usize::from(direction >= 2)];
                    let mut lower = center.wrapping_sub(1);
                    let mut upper = center.wrapping_add(1);
                    while lower >= 0 || upper < limit {
                        if lower >= 0 {
                            found = self
                                .navigation_cell(&mut remaining, block, lower, y)?
                                .filter(|object| enabled(object));
                            if found.is_some() {
                                break;
                            }
                        }
                        if upper < limit {
                            // 水平导航的侧向查找也使用此行坐标，保留基线入口的轴选择。
                            found = self
                                .navigation_cell(&mut remaining, block, upper, y)?
                                .filter(|object| enabled(object));
                            if found.is_some() {
                                break;
                            }
                        }
                        lower = lower.wrapping_sub(1);
                        upper = upper.wrapping_add(1);
                    }
                }
                if let Some(object) = found {
                    return Ok(Some(reference(object)));
                }
                x = x.wrapping_add(dx);
                y = y.wrapping_add(dy);
            }
            let point = self.navigation_position(origin).map(|value| value as f32);
            let mut neighbor = grid.neighbors[direction];
            let mut nearest = None;
            while let Some(grid) = usize::try_from(neighbor).ok().and_then(|i| blocks.get(i)) {
                consume(&mut remaining)?;
                let mut distance = 100_000_000.0f32;
                for object in self.bound_buttons().filter(|object| enabled(object)) {
                    let ObjectState::Button(button) = &object.state else {
                        unreachable!()
                    };
                    if button.keyblock[0] != neighbor {
                        continue;
                    }
                    let at = self
                        .navigation_position(object.handle)
                        .map(|value| value as f32);
                    let delta = [at[0] - point[0], at[1] - point[1]];
                    let value = delta[1] * delta[1] + delta[0] * delta[0];
                    if value < distance {
                        distance = value;
                        nearest = Some(object);
                    }
                }
                if nearest.is_some() {
                    break;
                }
                neighbor = grid.neighbors[direction];
            }
            let Some(object) = nearest else {
                return Ok(None);
            };
            let ObjectState::Button(button) = &object.state else {
                unreachable!()
            };
            [block, x, y] = button.keyblock;
            origin = object.handle;
        }
    }

    fn navigation_cell(
        &self,
        remaining: &mut usize,
        block: i32,
        x: i32,
        y: i32,
    ) -> Result<Option<&Object>> {
        consume(remaining)?;
        Ok(self.bound_buttons().find(|object| {
            let ObjectState::Button(button) = &object.state else {
                unreachable!()
            };
            button.keyblock == [block, x, y]
        }))
    }

    fn bound_buttons(&self) -> impl Iterator<Item = &Object> {
        self.bindings
            .values()
            .flat_map(|banks| banks.values())
            .filter_map(|handle| self.get(*handle))
            .filter(|object| matches!(object.state, ObjectState::Button(_)))
    }

    fn navigation_position(&self, handle: u32) -> [i32; 2] {
        self.image_position(handle).map(integer_position)
    }
}

fn integer_position(value: f32) -> i32 {
    if (-2147483648.0..2147483648.0).contains(&value) {
        value as i32
    } else {
        i32::MIN
    }
}

fn enabled(object: &Object) -> bool {
    matches!(&object.state, ObjectState::Button(button) if button.enabled != 0)
}

fn reference(object: &Object) -> fes::ObjectReference {
    fes::ObjectReference {
        name: object.name.clone(),
        bank: object.bank as i32,
    }
}

fn consume(remaining: &mut usize) -> Result<()> {
    *remaining = remaining
        .checked_sub(1)
        .ok_or_else(|| Error::new(0, "FES 导航搜索超过查询上限"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        graphics::Image,
        hg3::{DecodedFrame, StandardInfo},
        kcs::{Host, Interpreter, NativeReturn},
    };

    struct Pixels(Image);
    impl Host for Pixels {
        fn call(&mut self, _: u32, _: &[u8], _: &mut Interpreter) -> Result<NativeReturn> {
            unreachable!()
        }
        fn image(&self, handle: u32) -> Result<&Image> {
            assert_eq!(handle, 123);
            Ok(&self.0)
        }
    }

    fn add(objects: &mut Objects, kind: &'static str, name: &str) -> u32 {
        objects
            .define(&fes::ObjectDefinition {
                kind,
                name: name.into(),
                banks: 1,
            })
            .unwrap()[0]
    }

    #[test]
    fn navigation_geometry_keeps_record_hits_pixel_offsets_and_extended_plane_positions_distinct() {
        let host = Pixels(
            Image::from_frame(DecodedFrame {
                id: 0,
                info: StandardInfo {
                    size: [3, 2],
                    offset: [2, 1],
                    canvas_size: [8, 6],
                    base: [0; 2],
                    bit_depth: 32,
                    reserved: 0,
                },
                pixels: [0u32, 0x01000000, 0xff000000, 0x80000000, 0, 0x7f000000]
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect(),
            })
            .unwrap(),
        );
        let mut objects = Objects::new(crate::HandleAllocator::default()).unwrap();
        let first = add(&mut objects, "BUTTON", "z_first");
        let second = add(&mut objects, "BUTTON", "a_second");
        for handle in [first, second] {
            let image = objects.get_mut(handle).unwrap().state.image_mut().unwrap();
            image.position = [1.75, 2.75];
            image.base_point = [0.5, -0.5];
            image.set_size([5, 4]);
        }
        let object = objects.get_mut(first).unwrap();
        object.image = Some(123);
        let ObjectState::Button(button) = &mut object.state else {
            unreachable!()
        };
        button.mask_type = 1;
        button.enabled = 0;
        let first_target = reference(objects.get(first).unwrap());
        let second_target = reference(objects.get(second).unwrap());
        for (point, expected) in [
            ([4, 4], Some(first_target.clone())),
            ([3, 4], Some(second_target.clone())),
            ([5, 5], Some(first_target.clone())),
            ([2, 3], Some(second_target)),
            ([6, 4], None),
        ] {
            assert_eq!(objects.navigation_hit(point, &host).unwrap(), expected);
        }
        objects.get_mut(first).unwrap().image = None;
        assert_eq!(
            objects.navigation_hit([3, 4], &host).unwrap(),
            Some(first_target.clone())
        );

        let view = add(&mut objects, "CPLANE", "view");
        let parent = add(&mut objects, "PLANE", "parent");
        objects.set_parent(view, Some(parent)).unwrap();
        for (handle, position) in [(view, [10.75, -8.75]), (parent, [1.5, -1.5])] {
            let plane = objects.get_mut(handle).unwrap().state.plane_mut().unwrap();
            plane.position = [position[0], position[1], 0., 1.];
            plane.base_point = [100., 99., 0., 0.];
        }
        let plane_reference = reference(objects.get(view).unwrap());
        let image = objects.get_mut(first).unwrap().state.image_mut().unwrap();
        image.plane = plane_reference;
        image.set_key_position(-7, [8, -9]);
        assert_eq!(objects.navigation_pointer(&first_target), Some([9, -6]));
        objects
            .get_mut(view)
            .unwrap()
            .state
            .plane_mut()
            .unwrap()
            .extended_position = -3;
        assert_eq!(objects.navigation_pointer(&first_target), Some([21, -16]));
        objects
            .get_mut(first)
            .unwrap()
            .state
            .image_mut()
            .unwrap()
            .set_key_position(1, [i32::MAX, i32::MIN]);
        objects
            .get_mut(view)
            .unwrap()
            .state
            .plane_mut()
            .unwrap()
            .position = [1e20, -1e20, 0., 1.];
        assert_eq!(objects.navigation_pointer(&first_target), Some([0, 3]));
    }
}
