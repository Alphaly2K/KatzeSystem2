use super::{ObjectState, Objects};
use crate::{Error, Result, expression::State, fes};

enum Command {
    Display(i32),
    Size([i32; 2]),
    KeyPosition(i32, [i32; 2]),
    SetId(i32),
    GetId(i32),
    Enable(i32),
    Priority(i32),
    Position(fes::ImageReference),
}

impl Objects {
    /// 消费已支持的对象属性及运动状态命令；其他命令返回 false。
    /// 属性请求在后续对象更新时生效；不执行目标缺失时的整行表达式回退。
    pub fn dispatch_object_command(
        &mut self,
        interpreter: &mut fes::Interpreter,
        line: &fes::Line,
        state: &mut State,
    ) -> Result<bool> {
        let args = interpreter.parameters(&line.bytes, state)?;
        let Some(name) = args.get(1).map(Vec::as_slice) else {
            return Ok(false);
        };
        if !matches!(
            name,
            b"disp"
                | b"size"
                | b"keypos"
                | b"setid"
                | b"getid"
                | b"enable"
                | b"pri"
                | b"pos2"
                | b"delay"
                | b"ismot"
        ) {
            return Ok(false);
        }
        let mut target = if args[0] == b"@" {
            let context = interpreter
                .event_context()
                .ok_or_else(|| Error::new(line.offset, "FES 当前没有事件对象"))?;
            fes::ObjectReference {
                name: context.object.clone(),
                bank: context.bank,
            }
        } else {
            interpreter.object_reference(&args[0], state)?
        };
        if self
            .lookup(
                &target.name,
                if target.bank == -1 { 0 } else { target.bank },
            )
            .is_none()
        {
            return Err(Error::new(
                line.offset,
                "FES 对象目标不存在，整行表达式回退尚未接入",
            ));
        }
        if matches!(name, b"delay" | b"ismot") {
            return self.dispatch_motion_metadata(interpreter, line.offset, &args, &target, state);
        }
        if self.count(&target.name) == 1 {
            target.bank = 0;
        }
        if name == b"getid" && target.bank == -1 {
            return Ok(true);
        }
        let mut number = |index: usize, default| {
            args.get(index).map_or(Ok(default), |bytes| {
                interpreter
                    .eval(bytes, state)
                    .map_err(|error| Error::new(line.offset + error.offset, error.message))
            })
        };
        let command = match name {
            b"disp" => Command::Display(i32::from(number(2, 1)? != 0)),
            b"size" => Command::Size([number(2, 0)?, number(3, 0)?]),
            b"keypos" => Command::KeyPosition(number(2, 0)?, [number(3, 0)?, number(4, 0)?]),
            b"setid" => Command::SetId(number(2, 0)?),
            b"getid" => Command::GetId(number(2, 0)?),
            b"enable" => Command::Enable(i32::from(number(2, 0)? != 0)),
            b"pri" if args.len() > 2 => Command::Priority(number(2, 0)?),
            b"pos2" if args.len() > 2 => {
                let frame = number(3, 0)?;
                let file = &args[2];
                let end = file.iter().position(|&b| b == 0).unwrap_or(file.len());
                let mut reference = file[..end].to_vec();
                reference.extend(format!(":{frame}").bytes());
                let reference = fes::ImageReference::parse(&reference);
                if reference.name.len() >= 128 {
                    return Err(Error::new(line.offset, "FES POS2 名称超过存储上限"));
                }
                Command::Position(reference)
            }
            _ => return Ok(true),
        };
        if let Command::GetId(index) = command {
            let handle = self.lookup(&target.name, target.bank).unwrap();
            let Some(image) = self.get(handle).unwrap().state.image_source() else {
                return Err(Error::new(
                    line.offset,
                    "FES getid 目标类型不支持可移植的编号查询",
                ));
            };
            if let Some(value) = state.shared.get_mut(index as usize) {
                *value = image.image_number;
            }
            return Ok(true);
        }
        let banks = if target.bank == -1 {
            0..i64::from(self.count(&target.name))
        } else {
            i64::from(target.bank)..i64::from(target.bank) + 1
        };
        for bank in banks {
            let Some(handle) = self.lookup(&target.name, bank as i32) else {
                if matches!(command, Command::Position(_) | Command::Enable(_)) {
                    continue;
                }
                break;
            };
            let object = self.get_mut(handle).unwrap();
            let mut dirty = false;
            match &command {
                Command::Display(value) => {
                    if let Some(image) = object.state.image_mut() {
                        dirty = image.display != *value;
                        image.display = *value;
                    } else if let Some(plane) = object.state.plane_mut() {
                        dirty = plane.display != *value;
                        plane.display = *value;
                    }
                }
                Command::Size(size) => {
                    if let Some(image) = object.state.image_mut() {
                        image.set_size(*size);
                    } else if let Some(plane) = object.state.plane_mut() {
                        plane.size = *size;
                        plane.rebuild = true;
                    } else {
                        break;
                    }
                }
                Command::KeyPosition(mode, position) => {
                    if let Some(image) = object.state.image_mut() {
                        image.set_key_position(*mode, *position);
                    }
                }
                Command::SetId(value) => {
                    if let Some(image) = object.state.image_source_mut() {
                        image.apply(
                            &fes::ObjectAttributes {
                                image_number: Some(*value),
                                ..Default::default()
                            },
                            bank as u32,
                            |name| Ok(name.to_vec()),
                        )?;
                    }
                }
                Command::Priority(value) => {
                    if let Some(image) = object.state.image_mut() {
                        image.set_priority(*value);
                    } else if let Some(plane) = object.state.plane_mut() {
                        plane.priority = *value;
                        dirty = true;
                    }
                }
                Command::Position(reference) => {
                    if let Some(image) = object.state.image_mut() {
                        image.position_resource = Some(reference.clone());
                        dirty = true;
                    }
                }
                Command::Enable(value) => {
                    if let ObjectState::Button(button) = &mut object.state {
                        button.image.base.updates.dirty |= button.enabled != *value;
                        button.enabled = *value;
                    }
                }
                Command::GetId(_) => unreachable!(),
            }
            if dirty {
                self.invalidate(handle)?;
            }
        }
        Ok(true)
    }

    fn dispatch_motion_metadata(
        &mut self,
        interpreter: &mut fes::Interpreter,
        offset: usize,
        args: &[Vec<u8>],
        target: &fes::ObjectReference,
        state: &mut State,
    ) -> Result<bool> {
        let delay = args[1] == b"delay";
        if !delay && args.len() < 3 {
            return Ok(true);
        }
        let value = args.get(2).map_or(Ok(0), |bytes| {
            interpreter
                .eval(bytes, state)
                .map_err(|error| Error::new(offset + error.offset, error.message))
        })?;
        let banks = if target.bank == -1 {
            0..i64::from(self.count(&target.name))
        } else {
            i64::from(target.bank)..i64::from(target.bank) + 1
        };
        let mut active = false;
        for bank in banks {
            let Some(handle) = self.lookup(&target.name, bank as i32) else {
                return Ok(true);
            };
            if delay {
                self.get_mut(handle).unwrap().motion_delay = value as u32;
            } else {
                active |= self.motion_active(handle)?;
            }
        }
        if !delay {
            if let Some(slot) = state.shared.get_mut(value as usize) {
                *slot = i32::from(active);
            }
        }
        Ok(true)
    }
}
