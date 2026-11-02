use super::{Object, ObjectState, Objects};
use crate::{Error, Result, fes};

impl Objects {
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
        self.image_position(handle).map(|value| {
            if (-2147483648.0..2147483648.0).contains(&value) {
                value as i32
            } else {
                i32::MIN
            }
        })
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
