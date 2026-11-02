use super::Scene;
use crate::{Error, Result, expression::State};

impl Scene {
    pub(super) fn wait_command(
        &mut self,
        args: &[Vec<u8>],
        offset: usize,
        now: u32,
        numbers: &mut State,
    ) -> Result<Option<u32>> {
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
            b"tag" | b"string" | b"privateflag" => {
                return Err(Error::new(offset, "FES 此类完成等待尚未接入"));
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
            -1 => {
                if !self.objects.motion_active(self.objects.top())? {
                    self.deadline = Some(0);
                } else if self.input.left {
                    self.objects.request_motion_finish(self.objects.top())?;
                }
                return Ok(true);
            }
            -2 if now.wrapping_sub(self.frame_origin) >= self.frame_wait => return Ok(false),
            -2 => (),
            -4 => {
                if self.input.left {
                    return Ok(false);
                }
                if let Some(target) = &self.wait_target {
                    let banks = if target.bank == -1 {
                        0..i64::from(self.objects.count(&target.name))
                    } else {
                        i64::from(target.bank)..i64::from(target.bank) + 1
                    };
                    for bank in banks {
                        if let Some(handle) = self.objects.lookup(&target.name, bank as i32) {
                            if self.objects.motion_active(handle)? {
                                return Ok(true);
                            }
                        }
                    }
                }
                return Ok(false);
            }
            -7..=-3 => {
                return Err(Error::new(0, "FES 此类完成等待尚未接入"));
            }
            _ if now >= deadline => return Ok(false),
            _ => (),
        }
        if self.input.left {
            self.objects.request_motion_finish(self.objects.top())?;
            Ok(false)
        } else {
            Ok(true)
        }
    }
}
