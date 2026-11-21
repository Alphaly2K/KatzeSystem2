use super::{Notification, Scene, ScriptRequest};
use crate::runtime::fes::ObjectState;
use crate::{Result, expression::State};

impl Scene {
    pub(super) fn key_skip_command(
        &mut self,
        args: &[Vec<u8>],
        numbers: &mut State,
    ) -> Result<bool> {
        if args.first().is_none_or(|name| name != b"keyskip") {
            return Ok(false);
        }
        let (mut enabled, mut ctrl, mut continuous) = (true, false, false);
        let mut index = 1;
        while let Some(value) = args.get(index) {
            if value.eq_ignore_ascii_case(b"on") {
                enabled = true;
            } else if value.eq_ignore_ascii_case(b"off") {
                enabled = false;
            } else if value.eq_ignore_ascii_case(b"ctrl") {
                ctrl = true;
            } else if value.eq_ignore_ascii_case(b"cont") {
                continuous = true;
            } else if value.eq_ignore_ascii_case(b"force") {
                index += 1;
                self.input.forced = args
                    .get(index)
                    .map_or(Ok(0), |bytes| self.interpreter.eval(bytes, numbers))?
                    != 0;
                if self.input.forced {
                    self.input.left = true;
                    self.input.mode = true;
                    self.set_system_value("_CLICK_L_", -1);
                }
            } else {
                enabled = self.interpreter.eval(value, numbers)? != 0;
            }
            index += 1;
        }
        self.input.cancel_enabled = enabled;
        self.input.ctrl = ctrl;
        self.input.continuous = continuous;
        Ok(true)
    }

    pub(super) fn wait_command(
        &mut self,
        args: &[Vec<u8>],
        now: u32,
        numbers: &mut State,
    ) -> Result<Option<u32>> {
        if args.first().is_some_and(|name| name == b"m2_waiteff") {
            let Some(reference) = args.get(1) else {
                return Ok(None);
            };
            let target = self.interpreter.object_reference(reference, numbers)?;
            let bank = if target.bank == -1 { 0 } else { target.bank };
            self.wait_target = self.objects.lookup(&target.name, bank).map(|_| target);
            return Ok(Some(u32::MAX - 4));
        }
        if args.first().is_none_or(|name| name != b"wait") {
            return Ok(None);
        }
        let Some(value) = args.get(1) else {
            return Ok(Some(u32::MAX));
        };
        match value.as_slice() {
            b"frame" => {
                self.frame_wait = args
                    .get(2)
                    .map_or(Ok(0), |bytes| self.interpreter.eval(bytes, numbers))?
                    as u32;
                return Ok(Some(u32::MAX - 1));
            }
            b"privateflag" if args.get(2).is_some() => {
                self.wait_private_flag = self.interpreter.eval(&args[2], numbers)? as u32;
                return Ok(Some(u32::MAX - 6));
            }
            b"string" => {
                if let Some(reference) = args.get(2) {
                    let target = self.interpreter.object_reference(reference, numbers)?;
                    if self
                        .objects
                        .lookup(
                            &target.name,
                            if target.bank == -1 { 0 } else { target.bank },
                        )
                        .is_some()
                    {
                        self.wait_target = Some(target);
                        return Ok(Some(u32::MAX - 5));
                    }
                }
                let duration = self.interpreter.eval_fallback(value, numbers)?.max(1) as u32;
                return Ok(Some(now.wrapping_add(duration)));
            }
            b"privateflag" => {
                let duration = self.interpreter.eval_fallback(value, numbers)?.max(1) as u32;
                return Ok(Some(now.wrapping_add(duration)));
            }
            b"tag" => {
                let name = args.get(2).map_or(&[][..], Vec::as_slice);
                let end = name
                    .iter()
                    .take(32)
                    .position(|&byte| byte == 0)
                    .unwrap_or(name.len().min(32));
                let mut name = name[..end].to_vec();
                name.push(0);
                self.frame_wait = 0;
                self.notify(Notification::Script(ScriptRequest::WaitTag { name }))?;
                return Ok(Some(u32::MAX - 2));
            }
            _ => (),
        }
        let target = self.interpreter.object_reference(value, numbers)?;
        if self
            .objects
            .lookup(
                &target.name,
                if target.bank == -1 { 0 } else { target.bank },
            )
            .is_some()
        {
            self.wait_target = Some(target);
            return Ok(Some(u32::MAX - 3));
        }
        let duration = self.interpreter.eval(value, numbers)?.max(1) as u32;
        Ok(Some(now.wrapping_add(duration)))
    }

    pub(super) fn waiting_at_start(&mut self, now: u32) -> Result<bool> {
        let Some(deadline) = self.deadline else {
            return Ok(false);
        };
        match deadline as i32 {
            -7 => {
                return Ok(self.wait_private_flag < 512
                    && self.interpreter.local_values()[self.wait_private_flag as usize] == 0);
            }
            -6 => {
                let cancel = self.input.left
                    && (!self.input.mode || self.input.ctrl || self.input.cancel_enabled);
                if let Some(target) = &self.wait_target {
                    let banks = if target.bank == -1 {
                        0..i64::from(self.objects.count(&target.name) as i32)
                    } else {
                        i64::from(target.bank)..i64::from(target.bank) + 1
                    };
                    for bank in banks {
                        if let Some(handle) = self.objects.lookup(&target.name, bank as i32) {
                            if let ObjectState::String(state) =
                                &mut self.objects.get_mut(handle).unwrap().state
                            {
                                if state.remaining() != 0 {
                                    if cancel {
                                        state.request_skip();
                                    }
                                    return Ok(true);
                                }
                            }
                        }
                    }
                }
                return Ok(false);
            }
            -1 => {
                if !self.objects.motion_active(self.objects.top())? {
                    self.deadline = Some(0);
                } else if self.input.cancel_enabled && (self.input.mode || self.input.left) {
                    self.objects.request_motion_finish(self.objects.top())?;
                }
                return Ok(true);
            }
            -2 if now.wrapping_sub(self.frame_origin) >= self.frame_wait => return Ok(false),
            -2 => (),
            -4 => {
                if self.input.left
                    && (!self.input.mode || self.input.ctrl || self.input.cancel_enabled)
                {
                    return Ok(false);
                }
                if let Some(target) = &self.wait_target {
                    let banks = if target.bank == -1 {
                        0..i64::from(self.objects.count(&target.name) as i32)
                    } else {
                        i64::from(target.bank)..i64::from(target.bank) + 1
                    };
                    for bank in banks {
                        if let Some(handle) = self.objects.lookup(&target.name, bank as i32) {
                            let active = match &self.objects.get(handle).unwrap().state {
                                ObjectState::Particle(particle) => particle.playing(),
                                _ => self.objects.motion_active(handle)?,
                            };
                            if active {
                                return Ok(true);
                            }
                        }
                    }
                }
                return Ok(false);
            }
            -5 => {
                let Some(target) = &self.wait_target else {
                    return Ok(false);
                };
                let banks = if target.bank == -1 {
                    0..i64::from(self.objects.count(&target.name))
                } else {
                    i64::from(target.bank)..i64::from(target.bank) + 1
                };
                return Ok(banks
                    .filter_map(|bank| self.objects.lookup(&target.name, bank as i32))
                    .any(|handle| {
                        matches!(
                            &self.objects.get(handle).unwrap().state,
                            ObjectState::ModelPlane(model) if model.effects_active
                        )
                    }));
            }
            _ if now >= deadline => return Ok(false),
            _ => (),
        }
        if self.input.cancel_enabled
            && (self.input.left || (deadline == u32::MAX - 1 && self.input.mode))
        {
            self.objects.request_motion_finish(self.objects.top())?;
            Ok(false)
        } else {
            Ok(true)
        }
    }
}
