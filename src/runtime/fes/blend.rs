use super::{ObjectState, Objects, Scene};
use crate::runtime::{ObjectFactory, Store};
use crate::{Error, Result, fes};

pub(super) struct BlendCall {
    target: fes::ObjectReference,
    value: i32,
    bank: i64,
    end: i64,
    prepared: Option<PreparedBlend>,
}

struct PreparedBlend {
    handle: u32,
    motion_name: Option<Vec<u8>>,
    parameters: [f32; 4],
}

impl Objects {
    fn apply_blend(&mut self, handle: u32, value: i32) -> Result<bool> {
        self.require_motion_idle()?;
        let state = &mut self.get_mut(handle).unwrap().state;
        let dirty = if let Some(image) = state.image_mut() {
            let changed = image.blend != value;
            image.blend = value;
            changed
        } else if let Some(control) = state.control_mut() {
            control.set_alpha(value as u32);
            false
        } else if let Some(plane) = state.plane_mut() {
            plane.color = (plane.color & 0xffffff) | ((value as u32) << 24);
            true
        } else {
            match state {
                ObjectState::Sound(sound) => {
                    let changed = sound.volume != value;
                    sound.volume = value;
                    changed
                }
                ObjectState::Base | ObjectState::Draw(_) => return Ok(false),
                _ => return Err(Error::new(0, "FES 此对象类型的 blend 尚未接入")),
            }
        };
        if dirty {
            self.invalidate(handle)?;
        }
        self.capture_motion_base(handle)?;
        Ok(true)
    }
}

impl<F: ObjectFactory> Store<'_, F> {
    pub(super) fn run_fes_blend(
        &mut self,
        scene: &mut Scene,
        args: &[Vec<u8>],
        offset: usize,
    ) -> Result<Option<bool>> {
        if args.get(1).is_none_or(|name| name != b"blend") {
            return Ok(None);
        }
        let result = (|| {
            if scene.blend_call.is_none() {
                let target = if args[0] == b"@" {
                    let context = scene
                        .interpreter
                        .event_context()
                        .ok_or_else(|| Error::new(0, "FES 当前没有事件对象"))?;
                    fes::ObjectReference {
                        name: context.object.clone(),
                        bank: context.bank,
                    }
                } else {
                    scene.interpreter.object_reference(&args[0], self.numbers)?
                };
                if scene.objects.count(&target.name) == 0 {
                    return Err(Error::new(0, "FES 对象目标不存在，整行表达式回退尚未接入"));
                }
                let value = args
                    .get(2)
                    .map_or(Ok(255), |input| scene.interpreter.eval(input, self.numbers))?;
                let (bank, end) = if target.bank == -1 {
                    (0, i64::from(scene.objects.count(&target.name)))
                } else {
                    (i64::from(target.bank), i64::from(target.bank) + 1)
                };
                scene.blend_call = Some(BlendCall {
                    target,
                    value,
                    bank,
                    end,
                    prepared: None,
                });
            }
            for _ in 0..self.budget.max(1) {
                let pending = scene.blend_call.as_mut().unwrap();
                if pending.prepared.is_none() {
                    if let Some(handle) = scene
                        .objects
                        .lookup(&pending.target.name, pending.bank as i32)
                    {
                        if scene.objects.apply_blend(handle, pending.value)? {
                            let mut parameters = [pending.value as f32, 255.0, 255.0, 255.0];
                            if let Some(plane) = scene.objects.get(handle).unwrap().state.plane() {
                                for (parameter, shift) in parameters[1..].iter_mut().zip([16, 8, 0])
                                {
                                    *parameter = ((plane.color() >> shift) & 255) as f32;
                                }
                            }
                            pending.prepared = Some(PreparedBlend {
                                handle,
                                motion_name: None,
                                parameters,
                            });
                        }
                    }
                }
                if let Some(prepared) = &mut pending.prepared {
                    if prepared.motion_name.is_none() {
                        let services = self
                            .resources
                            .services
                            .as_deref_mut()
                            .ok_or_else(|| Error::new(0, "运行时未配置 FES 运动资源服务"))?;
                        prepared.motion_name = Some(services.resolve(b"mot", b"p")?);
                    }
                    if let Some(program) = self
                        .resources
                        .load_motion(prepared.motion_name.as_ref().unwrap())?
                    {
                        let delay = scene.objects.get(prepared.handle).unwrap().motion_delay();
                        scene.objects.insert_motion(
                            prepared.handle,
                            program,
                            crate::kcs::MotionState::with_parameters(&prepared.parameters)?,
                            delay,
                        )?;
                    }
                }
                pending.prepared = None;
                pending.bank += 1;
                if pending.bank == pending.end {
                    scene.blend_call = None;
                    return Ok(Some(true));
                }
            }
            Ok(Some(false))
        })();
        result.map_err(|error: Error| Error::new(offset + error.offset, error.message))
    }
}
