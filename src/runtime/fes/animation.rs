use super::{ObjectState, Objects};
use crate::{
    Error, Result,
    expression::State,
    fes,
    runtime::{ObjectFactory, Runtime},
};

/// FES IMAGE 保存的动画请求；待对象更新时装载并设置列表播放状态。
#[derive(Clone, Debug, Default)]
pub struct ImageAnimationRequest {
    name: Vec<u8>,
    playing: i32,
    pending: bool,
}

impl ImageAnimationRequest {
    pub fn name(&self) -> &[u8] {
        &self.name
    }

    pub fn playing(&self) -> i32 {
        self.playing
    }

    pub fn is_pending(&self) -> bool {
        self.pending
    }
}

impl Objects {
    /// 消费 setanm／anm 内建命令；其他命令返回 false，调用方只在成功消费后确认事件。
    pub fn dispatch_image_animation(
        &mut self,
        interpreter: &mut fes::Interpreter,
        line: &fes::Line,
        state: &mut State,
    ) -> Result<bool> {
        let args = interpreter.parameters(&line.bytes, state)?;
        let Some(command) = args.get(1).map(Vec::as_slice) else {
            return Ok(false);
        };
        if !matches!(command, b"setanm" | b"anm") {
            return Ok(false);
        }
        let target = if args[0] == b"@" {
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
        let value = args.get(2).map_or(&[][..], Vec::as_slice);
        let value = &value[..value.iter().position(|&b| b == 0).unwrap_or(value.len())];
        let name = if command == b"setanm" {
            let mut name = value.to_vec();
            let end = extension_start(&name);
            if end == name.len() || end + 1 == name.len() {
                name.truncate(end);
                name.extend(b".anm");
            }
            if name.len() >= 256 {
                return Err(Error::new(line.offset, "FES 动画名称超过存储上限"));
            }
            name
        } else {
            Vec::new()
        };
        let banks = if target.bank == -1 {
            0..i64::from(self.count(&target.name))
        } else {
            i64::from(target.bank)..i64::from(target.bank) + 1
        };
        for bank in banks {
            if let Some(handle) = self.lookup(&target.name, bank as i32) {
                let object = self.get_mut(handle).unwrap();
                if matches!(object.state, ObjectState::Image(_)) {
                    if command == b"setanm" {
                        object.animation.name.clone_from(&name);
                    } else {
                        object.animation.playing = i32::from(!value.eq_ignore_ascii_case(b"STOP"));
                    }
                    object.animation.pending = true;
                }
            }
        }
        Ok(true)
    }
}

impl<F: ObjectFactory> Runtime<F> {
    pub(super) fn update_fes_image_animation(
        &mut self,
        owner: u32,
        objects: &mut Objects,
        handle: u32,
        list: u32,
    ) -> Result<()> {
        let object = objects.get_mut(handle).unwrap();
        if !object.animation.pending {
            return Ok(());
        }
        object.animation.pending = false;
        let playing = object.animation.playing != 0;
        let slot = object.state.image().unwrap().parent_priority as u32;
        if playing {
            let services = self
                .resources
                .services
                .as_deref_mut()
                .ok_or_else(|| Error::new(0, "运行时未配置 FES 动画资源服务"))?;
            let mut name = services.resolve(b"image", &object.animation.name)?;
            let end = name.iter().position(|&b| b == 0).unwrap_or(name.len());
            name.truncate(end);
            name.truncate(extension_start(&name));
            name.extend(b".anm\0");
            self.fes_command(owner, Some(list), 299, [slot, 0, 0], Some(&mut name))?;
        }
        self.fes_command(owner, Some(list), 301, [slot, u32::from(playing), 0], None)
    }
}

fn extension_start(name: &[u8]) -> usize {
    name.iter()
        .rposition(|b| matches!(b, b'.' | b'/' | b'\\'))
        .filter(|&i| name[i] == b'.')
        .unwrap_or(name.len())
}
