use super::{HostState, Interpreter, NativeReturn, native};
use crate::{Error, Result, read};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Mahjong {
    handles: BTreeMap<u32, Vec<u32>>,
    settings: Settings,
    seats: usize,
    points: [u32; 4],
    remaining: u32,
}

#[derive(Default)]
struct Settings {
    seats: usize,
    rules: u32,
    first_dealer: u32,
    round_options: [u32; 3],
    seat_options: [u32; 4],
    starting_points: [u32; 4],
}

impl Mahjong {
    fn configure(&mut self, args: &[u32], vm: &Interpreter) -> Result<()> {
        let [
            handle,
            seats,
            rules,
            first_dealer,
            round1,
            round2,
            round3,
            options,
            points,
        ] = <[u32; 9]>::try_from(args).unwrap();
        let count = (seats as i32).saturating_sub(1).max(0) as usize;
        if count > read::MAX_ITEMS {
            return Err(Error::new(vm.pc, "麻将座位参数超过上限"));
        }
        let options = if count == 0 {
            None
        } else {
            Some(vm.read(options, count * 4)?)
        };
        let points = vm.read(points, 16)?;
        let packet = self.handles.get_mut(&handle).unwrap();
        packet.clear();
        packet.extend([
            0,
            seats,
            1,
            rules,
            2,
            first_dealer,
            3,
            round1,
            round2,
            round3,
        ]);

        let settings = &mut self.settings;
        settings.seats = if seats == 2 { 2 } else { 4 };
        settings.rules = rules;
        settings.first_dealer = first_dealer;
        settings.round_options = [round1, round2, round3];
        if let Some(options) = options {
            for (seat, word) in options.chunks_exact(4).enumerate() {
                let seat = seat + 1;
                let value = u32::from_le_bytes(word.try_into().unwrap());
                packet.extend([4, seat as u32, value]);
                if seat < settings.seats {
                    settings.seat_options[seat] = value;
                }
            }
        }
        for (seat, word) in points.chunks_exact(4).enumerate() {
            let value = u32::from_le_bytes(word.try_into().unwrap());
            if (value as i32) > 0 {
                packet.extend([5, seat as u32, value]);
                if seat < settings.seats {
                    settings.starting_points[seat] = value;
                }
            }
        }

        self.seats = settings.seats;
        self.points[..self.seats].copy_from_slice(&settings.starting_points[..self.seats]);
        self.remaining = if self.seats == 2 { 61 } else { 122 };
        Ok(())
    }
}

pub(super) fn call(
    host: &mut HostState,
    index: u32,
    arguments: &[u8],
    vm: &mut Interpreter,
) -> Result<NativeReturn> {
    use NativeReturn::{Continue, Value};
    if index == 470 {
        if host.mahjong.handles.len() >= read::MAX_ITEMS {
            return Err(Error::new(vm.pc, "麻将句柄数量超过上限"));
        }
        let handle = host.handle_allocator().allocate_at(vm.pc)?;
        host.mahjong.handles.insert(handle, Vec::new());
        return Ok(Value(handle));
    }
    let handle = read::u32_at(arguments, 0)?;
    if handle == 0 {
        return Ok(if index <= 472 { Continue } else { Value(0) });
    }
    let count = match index {
        472 => 9,
        480 | 495 => 2,
        _ => 1,
    };
    let args = native::words(arguments, count, vm.pc)?;
    let table = &mut host.mahjong;
    let packet = table
        .handles
        .get(&handle)
        .ok_or_else(|| Error::new(vm.pc, "麻将句柄无效"))?;
    Ok(match index {
        471 => {
            table.handles.remove(&handle);
            Continue
        }
        472 => {
            table.configure(&args, vm)?;
            Continue
        }
        475 => Value(table.seats as u32),
        480 => Value(
            packet
                .get(args[1].wrapping_add(1) as usize)
                .copied()
                .unwrap_or(0),
        ),
        481 => Value((packet.len() as u32).wrapping_sub(1)),
        492 => Value(table.remaining),
        495 => Value(
            table.points[..table.seats]
                .get(args[1] as usize)
                .copied()
                .unwrap_or(0),
        ),
        _ => unreachable!(),
    })
}
