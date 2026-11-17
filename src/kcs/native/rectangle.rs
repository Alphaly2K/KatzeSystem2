use super::words;
use crate::Result;
use crate::kcs::{Interpreter, NativeReturn};

fn read(vm: &Interpreter, address: u32) -> Result<[i32; 4]> {
    let bytes = vm.read(address, 16)?;
    Ok(std::array::from_fn(|i| {
        i32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
    }))
}

fn write(vm: &mut Interpreter, address: u32, rectangle: [i32; 4]) -> Result<()> {
    let bytes: Vec<_> = rectangle.into_iter().flat_map(i32::to_le_bytes).collect();
    vm.write(address, &bytes)
}

fn empty(rectangle: [i32; 4]) -> bool {
    rectangle[0] >= rectangle[2] || rectangle[1] >= rectangle[3]
}

pub(crate) fn unite_at(vm: &mut Interpreter, address: u32, other: [i32; 4]) -> Result<()> {
    let source = read(vm, address)?;
    let rectangle = match (empty(source), empty(other)) {
        (true, true) => [0; 4],
        (true, false) => other,
        (false, true) => source,
        (false, false) => [
            source[0].min(other[0]),
            source[1].min(other[1]),
            source[2].max(other[2]),
            source[3].max(other[3]),
        ],
    };
    write(vm, address, rectangle)
}

pub(super) fn call(index: u32, arguments: &[u8], vm: &mut Interpreter) -> Result<NativeReturn> {
    let count = match index {
        214 | 217 => 1,
        215 => 2,
        218 => 5,
        _ => unreachable!(),
    };
    let args = words(arguments, count, vm.pc)?;
    match index {
        214 => write(vm, args[0], [0; 4])?,
        215 => unite_at(vm, args[0], read(vm, args[1])?)?,
        217 => return Ok(NativeReturn::Value(u32::from(empty(read(vm, args[0])?)))),
        218 => {
            let source = read(vm, args[0])?;
            let mut rectangle = [
                source[0].max(args[1] as i32),
                source[1].max(args[2] as i32),
                source[2].min(args[3] as i32),
                source[3].min(args[4] as i32),
            ];
            if empty(rectangle) {
                rectangle = [0; 4];
            }
            write(vm, args[0], rectangle)?;
        }
        _ => unreachable!(),
    }
    Ok(NativeReturn::Continue)
}
