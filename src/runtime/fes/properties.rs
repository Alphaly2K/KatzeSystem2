use super::{ObjectState, Objects, Scene};
use crate::runtime::{ObjectFactory, Store};
use crate::{Error, Result, fes};

pub(super) struct PropertyCall {
    target: fes::ObjectReference,
    value: Property,
    bank: i64,
    end: i64,
    prepared: Option<PreparedProperty>,
}

enum Property {
    Blend(i32),
    Position([Option<i32>; 3]),
}

struct PreparedProperty {
    handle: u32,
    motion_name: Option<Vec<u8>>,
    parameters: Vec<f32>,
}

impl Objects {
    fn apply_position(
        &mut self,
        handle: u32,
        values: [Option<i32>; 3],
    ) -> Result<Option<Vec<f32>>> {
        self.require_motion_idle()?;
        let state = &mut self.get_mut(handle).unwrap().state;
        let parameters;
        let dirty;
        if let Some(image) = state.image_mut() {
            let next = std::array::from_fn(|i| {
                values[i].unwrap_or_else(|| super::plane::integer_position(image.position[i]))
                    as f32
            });
            dirty = image.position != next;
            if dirty {
                image.position = next;
            }
            parameters = vec![next[0], next[1], 0.0];
        } else if let Some(plane) = state.plane_mut() {
            let next = std::array::from_fn(|i| {
                if i == 3 {
                    1.0
                } else {
                    values[i].unwrap_or_else(|| super::plane::integer_position(plane.position[i]))
                        as f32
                }
            });
            dirty = plane.position != next;
            if dirty {
                plane.position = next;
            }
            parameters = next[..3].to_vec();
        } else {
            if let Some(control) = state.control_mut() {
                control.set_position(values[0].unwrap_or(0), 0);
            }
            return Ok(None);
        }
        if dirty {
            self.invalidate(handle)?;
        }
        self.capture_motion_base(handle)?;
        Ok(Some(parameters))
    }

    fn apply_property(&mut self, handle: u32, property: &Property) -> Result<Option<Vec<f32>>> {
        match property {
            Property::Position(values) => self.apply_position(handle, *values),
            Property::Blend(value) => {
                if !self.apply_blend(handle, *value)? {
                    return Ok(None);
                }
                let mut parameters = vec![*value as f32, 255.0, 255.0, 255.0];
                if let Some(plane) = self.get(handle).unwrap().state.plane() {
                    for (parameter, shift) in parameters[1..].iter_mut().zip([16, 8, 0]) {
                        *parameter = ((plane.color() >> shift) & 255) as f32;
                    }
                }
                Ok(Some(parameters))
            }
        }
    }

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
    pub(super) fn run_fes_property(
        &mut self,
        scene: &mut Scene,
        args: &[Vec<u8>],
        offset: usize,
    ) -> Result<Option<bool>> {
        let Some(name) = args.get(1) else {
            return Ok(None);
        };
        if !matches!(name.as_slice(), b"blend" | b"pos") {
            return Ok(None);
        }
        let result = (|| {
            if scene.property_call.is_none() {
                let mut target = if args[0] == b"@" {
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
                let count = scene.objects.count(&target.name);
                if count == 0 {
                    return Err(Error::new(0, "FES 对象目标不存在，整行表达式回退尚未接入"));
                }
                if name == b"pos" {
                    if scene
                        .objects
                        .lookup(
                            &target.name,
                            if target.bank == -1 { 0 } else { target.bank },
                        )
                        .is_none()
                    {
                        return Err(Error::new(0, "FES 对象目标不存在，整行表达式回退尚未接入"));
                    }
                    if count == 1 {
                        target.bank = 0;
                    }
                }
                let value = if name == b"blend" {
                    Property::Blend(
                        args.get(2)
                            .map_or(Ok(255), |input| scene.interpreter.eval(input, self.numbers))?,
                    )
                } else {
                    let mut values = [None; 3];
                    for (value, input) in values.iter_mut().zip(args.iter().skip(2)) {
                        *value = Some(scene.interpreter.eval(input, self.numbers)?);
                    }
                    Property::Position(values)
                };
                let (bank, end) = if target.bank == -1 {
                    (0, i64::from(count))
                } else {
                    (i64::from(target.bank), i64::from(target.bank) + 1)
                };
                scene.property_call = Some(PropertyCall {
                    target,
                    value,
                    bank,
                    end,
                    prepared: None,
                });
            }
            for _ in 0..self.budget.max(1) {
                let pending = scene.property_call.as_mut().unwrap();
                if pending.prepared.is_none() {
                    if let Some(handle) = scene
                        .objects
                        .lookup(&pending.target.name, pending.bank as i32)
                    {
                        if let Some(parameters) =
                            scene.objects.apply_property(handle, &pending.value)?
                        {
                            pending.prepared = Some(PreparedProperty {
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
                    scene.property_call = None;
                    return Ok(Some(true));
                }
            }
            Ok(Some(false))
        })();
        result.map_err(|error: Error| Error::new(offset + error.offset, error.message))
    }
}
