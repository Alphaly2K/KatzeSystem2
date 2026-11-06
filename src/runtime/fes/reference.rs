use super::{ObjectState, Objects};
use crate::runtime::{ObjectFactory, Runtime, Store};
use crate::{Error, Result, fes};

impl<F: ObjectFactory> Runtime<F> {
    /// 分派 refex；共享来源在命令阶段持有，图像列表在媒体更新时重新登记。
    pub fn dispatch_fes_reference(
        &mut self,
        objects: &mut Objects,
        interpreter: &mut fes::Interpreter,
        line: &fes::Line,
    ) -> Result<bool> {
        let args = interpreter.parameters(&line.bytes, self.numbers())?;
        self.store()
            .fes_reference_command(objects, interpreter, &args, line)
    }

    /// 释放独立 FES 对象存储的共享图像引用；任务内场景随任务回收自动处理。
    pub fn release_fes_image_references(&mut self, objects: &mut Objects) -> Result<()> {
        objects.release_image_references(&mut self.resources.images)
    }
}

impl Objects {
    pub(super) fn release_image_references(
        &mut self,
        images: &mut crate::graphics::Images,
    ) -> Result<()> {
        for object in self.objects.values_mut() {
            if let Some(image) = object.reference_image {
                images.release_reference(image)?;
                object.reference_image = None;
            }
        }
        Ok(())
    }
}

impl<F: ObjectFactory> Store<'_, F> {
    pub(super) fn fes_reference_command(
        &mut self,
        objects: &mut Objects,
        interpreter: &mut fes::Interpreter,
        args: &[Vec<u8>],
        line: &fes::Line,
    ) -> Result<bool> {
        if args.get(1).is_none_or(|name| name != b"refex") {
            return Ok(false);
        }
        objects.require_motion_idle()?;
        let mut target = interpreter.command_object_reference(&args[0], self.numbers)?;
        if objects
            .fallback_missing_target(interpreter, &target, &line.bytes, self.numbers)
            .map_err(|error| Error::new(line.offset + error.offset, error.message))?
        {
            return Ok(true);
        }
        if objects.count(&target.name) == 1 {
            target.bank = 0;
        }
        let source =
            args.get(2)
                .map_or(Ok(0), |value| interpreter.eval(value, self.numbers))? as u32;
        let banks = if target.bank == -1 {
            0..i64::from(objects.count(&target.name))
        } else {
            i64::from(target.bank)..i64::from(target.bank) + 1
        };
        for bank in banks {
            let Some(handle) = objects.lookup(&target.name, bank as i32) else {
                continue;
            };
            let object = objects.get_mut(handle).unwrap();
            match &object.state {
                ObjectState::CImage(_) | ObjectState::Image(_) => {
                    if source != 0 {
                        self.resources.images.get(source)?;
                    }
                    if let Some(previous) = object.reference_image {
                        self.resources.images.release_reference(previous)?;
                        object.reference_image = None;
                    }
                    if source != 0 {
                        self.resources.images.retain(source)?;
                        object.reference_image = Some(source);
                    }
                    object.state.image_mut().unwrap().updates.dirty = true;
                }
                ObjectState::DifImage(_) if source != 0 => {
                    return Err(Error::new(
                        line.offset,
                        "FES DIFIMAGE 的非零图像资源引用尚未接入",
                    ));
                }
                ObjectState::DifImage(_) => {
                    if let Some(previous) = object.image {
                        self.resources.images.release(previous)?;
                        object.image = None;
                        object.images.retain(|&image| image != previous);
                        for record in &mut objects.update_order {
                            if record.image == Some(previous) {
                                record.image = None;
                            }
                        }
                    }
                    objects.invalidate(handle)?;
                }
                _ => (),
            }
        }
        Ok(true)
    }
}
