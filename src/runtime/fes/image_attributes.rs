use super::{ObjectState, Objects};
use crate::runtime::{ObjectFactory, Runtime, Store};
use crate::{Error, Result, fes};

impl<F: ObjectFactory> Runtime<F> {
    /// 从当前图像来源查询 attr，向共享数值槽写入找到的五项记录。
    pub fn dispatch_fes_image_attributes(
        &mut self,
        objects: &Objects,
        interpreter: &mut fes::Interpreter,
        line: &fes::Line,
    ) -> Result<bool> {
        let args = interpreter.parameters(&line.bytes, self.numbers())?;
        self.store()
            .fes_image_attributes(objects, interpreter, &args, line)
    }
}

impl<F: ObjectFactory> Store<'_, F> {
    pub(super) fn fes_image_attributes(
        &mut self,
        objects: &Objects,
        interpreter: &mut fes::Interpreter,
        args: &[Vec<u8>],
        line: &fes::Line,
    ) -> Result<bool> {
        if args.get(1).is_none_or(|name| name != b"attr") {
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
        let Some(id) = args.get(2) else {
            return Ok(true);
        };
        if objects.count(&target.name) == 1 {
            target.bank = 0;
        }
        let id = interpreter.eval(id, self.numbers)? as u32;
        let mut outputs = [-1; 5];
        for (output, value) in outputs.iter_mut().zip(args.iter().skip(3)) {
            *output = interpreter.eval(value, self.numbers)?;
        }
        let banks = if target.bank == -1 {
            0..i64::from(objects.count(&target.name))
        } else {
            i64::from(target.bank)..i64::from(target.bank) + 1
        };
        for bank in banks {
            let Some(handle) = objects.lookup(&target.name, bank as i32) else {
                break;
            };
            let object = objects.get(handle).unwrap();
            let source = match &object.state {
                ObjectState::CImage(_)
                | ObjectState::Image(_)
                | ObjectState::ImageWindow(_)
                | ObjectState::Button(_) => object.reference_image.or(object.image),
                ObjectState::DifImage(_) => object.image,
                _ => None,
            };
            let Some(source) = source else { continue };
            if self.resources.images.pixel_handle(source)? == 0 {
                continue;
            }
            let Some(values) = self.resources.images.get(source)?.attribute(id) else {
                continue;
            };
            for (slot, value) in outputs.into_iter().zip(values) {
                if let Some(output) = self.numbers.shared.get_mut(slot as usize) {
                    *output = value as i32;
                }
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expression::State;
    use crate::graphics::Image;
    use crate::hg3::DecodedFrame;
    use crate::runtime::{BaseObject, TaskObject, kind};

    struct Reject;
    impl ObjectFactory for Reject {
        fn create(&mut self, _: u32, _: u32, _: &[u8]) -> Result<Box<dyn TaskObject>> {
            Err(Error::new(0, "合成工厂没有该类型"))
        }
    }

    #[test]
    fn queries_keep_source_precedence_parameter_effects_and_stop_at_bank_holes() {
        let mut runtime = Runtime::new(
            0,
            kind::ROOT,
            Box::new(BaseObject::new()),
            State::new(16),
            Reject,
        );
        let mut handles = Vec::new();
        for first in [1, 2] {
            let blank = Image::new([1, 1]).unwrap();
            let image = Image::from_frame(DecodedFrame {
                id: 0,
                info: *blank.info(),
                pixels: vec![0; 4],
                attributes: [(1, [first, u32::MAX, 0x80000000, 7, 9])].into(),
            })
            .unwrap();
            handles.push(runtime.resources.images.insert(0, image, None).unwrap());
        }
        let empty = runtime.images_mut().create_empty(0).unwrap();
        for kind in ["CIMAGE", "IMAGE", "BUTTON", "IMGWND", "DIFIMAGE"] {
            let mut objects = Objects::new(runtime.host().handle_allocator()).unwrap();
            let banks = objects
                .define(&fes::ObjectDefinition {
                    kind,
                    name: "item".into(),
                    banks: 3,
                })
                .unwrap();
            for &handle in &banks {
                objects.get_mut(handle).unwrap().image = Some(handles[0]);
            }
            if kind == "DIFIMAGE" {
                objects.get_mut(banks[1]).unwrap().image = Some(handles[1]);
            } else {
                objects.get_mut(banks[1]).unwrap().reference_image = Some(handles[1]);
            }
            objects.get_mut(banks[2]).unwrap().image = Some(empty);
            let mut interpreter =
                fes::Interpreter::new(fes::Program::from_text(b"#START\nwait 99\n").unwrap())
                    .unwrap();
            objects.publish_bindings(&mut interpreter);
            let mut send = |runtime: &mut Runtime<Reject>, objects: &Objects, bytes: &[u8]| {
                assert!(
                    runtime
                        .dispatch_fes_image_attributes(
                            objects,
                            &mut interpreter,
                            &fes::Line {
                                offset: 0,
                                bytes: bytes.to_vec()
                            }
                        )
                        .unwrap()
                );
            };
            runtime.numbers_mut().shared.fill(0);
            let before: Vec<_> = banks
                .iter()
                .map(|&h| objects.get(h).unwrap().state.clone())
                .collect();
            send(
                &mut runtime,
                &objects,
                b"item attr ($15+=1) ($14+=1) ($14+=1) ($14+=1) ($14+=1) ($14+=1) ($13+=100)",
            );
            assert_eq!(runtime.numbers().shared[1..6], [2, -1, i32::MIN, 7, 9]);
            assert_eq!(runtime.numbers().shared[13..], [0, 5, 1]);
            assert_eq!(
                before,
                banks
                    .iter()
                    .map(|&h| objects.get(h).unwrap().state.clone())
                    .collect::<Vec<_>>()
            );
            send(&mut runtime, &objects, b"item attr 65537 0 0 0 0 0");
            assert_eq!(runtime.numbers().shared[0], 9);
            send(&mut runtime, &objects, b"item attr 7 0 1 2 3 4");
            assert_eq!(runtime.numbers().shared[0], 9);
            objects.remove(banks[1]).unwrap();
            objects.get_mut(banks[2]).unwrap().image = Some(handles[1]);
            send(&mut runtime, &objects, b"item attr 1 0 1 2 3 4");
            assert_eq!(runtime.numbers().shared[0], 1);
            if kind != "DIFIMAGE" {
                objects.get_mut(banks[0]).unwrap().reference_image = Some(empty);
                runtime.numbers_mut().shared[0] = 77;
                send(&mut runtime, &objects, b"item attr 1 0 1 2 3 4");
                assert_eq!(runtime.numbers().shared[0], 77);
            }
            send(&mut runtime, &objects, b"item attr");
        }
    }
}
