use super::{ObjectState, Objects};
use crate::{Error, Result, expression::State, fes, kcs::ScriptRandom};

impl Objects {
    pub(super) fn dispatch_sound_table_command(
        &mut self,
        interpreter: &mut fes::Interpreter,
        offset: usize,
        args: &[Vec<u8>],
        target: &fes::ObjectReference,
        state: &mut State,
        mut random: Option<&mut ScriptRandom>,
    ) -> Result<bool> {
        let command = args[1].as_slice();
        let first = self
            .lookup(&target.name, target.bank.max(0))
            .and_then(|id| self.get(id));
        if !first.is_some_and(|object| object.state.kind() == "SNDTABLE") {
            return Ok(false);
        }
        if target.bank == -1 && !matches!(command, b"sndtbl" | b"setid") {
            return Ok(true);
        }
        let mut eval = |arg: usize, default: i32| {
            args.get(arg)
                .map_or(Ok(default), |value| interpreter.eval(value, state))
                .map_err(|error| Error::new(offset + error.offset, error.message))
        };
        let first = match command {
            b"next" | b"rev" => 0,
            b"getid" if args.len() < 3 => return Ok(true),
            _ => eval(2, 0)?,
        };
        let second = match command {
            b"sndtbl" => eval(4, 0)?,
            b"shuffle" => eval(3, -1)?,
            _ => 0,
        };
        let banks = if target.bank == -1 {
            0..i64::from(self.count(&target.name))
        } else {
            i64::from(target.bank)..i64::from(target.bank) + 1
        };
        for bank in banks {
            let Some(handle) = self.lookup(&target.name, bank as i32) else {
                break;
            };
            let ObjectState::Sound(sound) = &mut self.get_mut(handle).unwrap().state else {
                continue;
            };
            let Some(table) = sound.table.as_mut() else {
                continue;
            };
            let selection = match command {
                b"sndtbl" => {
                    table.set_track(first, args.get(3).map_or(b"", Vec::as_slice), second != 0)?;
                    None
                }
                b"setid" => Some(table.select(first)),
                b"getid" => {
                    if let Some(slot) = state.shared.get_mut(first as usize) {
                        *slot = table.id();
                    }
                    None
                }
                b"next" => Some(table.next()),
                b"rev" => Some(table.previous()),
                b"repeat" => {
                    table.repeat = first != 0;
                    None
                }
                b"shuffle" if first == 0 => {
                    table.reset_order();
                    None
                }
                b"shuffle" => table
                    .shuffle(
                        second,
                        random
                            .as_deref_mut()
                            .ok_or_else(|| Error::new(offset, "FES 曲目随机排序需要场景随机源"))?,
                        sound.action != 0,
                        sound.load_requested,
                    )
                    .map(fes::TrackSelection::Load),
                _ => unreachable!(),
            };
            match selection {
                Some(fes::TrackSelection::Load(file)) => {
                    sound.file = file;
                    sound.load_requested = true;
                }
                Some(fes::TrackSelection::Stop) if sound.action != 0 => {
                    sound.action = 0;
                    sound.action_requested = true;
                }
                _ => (),
            }
        }
        Ok(true)
    }
}
