use super::{Interpreter, NativeReturn, matrix, numeric};
use crate::{Result, read};

fn word(vm: &Interpreter, address: u32) -> Result<u32> {
    Ok(u32::from_le_bytes(vm.read(address, 4)?.try_into().unwrap()))
}

fn vector<const N: usize>(vm: &Interpreter, address: u32) -> Result<[f32; N]> {
    let bytes = vm.read(address, N * 4)?;
    Ok(std::array::from_fn(|i| {
        f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
    }))
}

fn write<const N: usize>(vm: &mut Interpreter, address: u32, value: [f32; N]) -> Result<()> {
    let bytes: Vec<_> = value.into_iter().flat_map(f32::to_le_bytes).collect();
    vm.write(address, &bytes)
}

pub(super) fn call(index: u32, arguments: &[u8], vm: &mut Interpreter) -> Result<NativeReturn> {
    let target = read::u32_at(arguments, 0)?;
    if index == 350 {
        if let Some(value) = matrix::inverse(vector::<16>(vm, target)?) {
            write(vm, target, value)?;
        }
        return Ok(NativeReturn::Continue);
    }
    if index == 351 {
        let source = read::u32_at(arguments, 4)?;
        // 原复制逐块读写，重叠区的后续块会读取先前写回的内容。
        for offset in [0, 16, 32, 48] {
            let bytes = vm.read(source.wrapping_add(offset), 16)?.to_vec();
            vm.write(target.wrapping_add(offset), &bytes)?;
        }
        return Ok(NativeReturn::Continue);
    }
    if matches!(index, 353 | 354) {
        let source = read::u32_at(arguments, 4)?;
        // 逐分量读取并写回，重叠来源可以看到先前分量的修改。
        for offset in [0, 4, 8, 12] {
            let left = word(vm, target.wrapping_add(offset))?;
            let right = word(vm, source.wrapping_add(offset))?;
            let (left, right) = if index == 353 && offset != 0 {
                (right, left)
            } else {
                (left, right)
            };
            let value = numeric::float(if index == 353 { 1 } else { 2 }, left, right);
            vm.write(target.wrapping_add(offset), &value.to_le_bytes())?;
        }
        return Ok(NativeReturn::Continue);
    }
    let value = vector::<4>(vm, target)?;
    let output = match index {
        352 => matrix::normalize4(value),
        355 | 356 => {
            let squares = value.map(|n| numeric::float(3, n.to_bits(), n.to_bits()));
            let square = squares[1..]
                .iter()
                .fold(squares[0], |sum, &n| numeric::float(1, sum, n));
            return Ok(NativeReturn::Value(if index == 355 {
                numeric::native_math(17, &[square])
            } else {
                square
            }));
        }
        357 => {
            let scale = word(vm, read::u32_at(arguments, 4)?)?;
            value.map(|n| f32::from_bits(numeric::float(3, n.to_bits(), scale)))
        }
        358 => {
            let matrix = vector::<16>(vm, read::u32_at(arguments, 4)?)?;
            std::array::from_fn(|col| {
                let products: [u32; 4] = std::array::from_fn(|row| {
                    numeric::float(3, matrix[row * 4 + col].to_bits(), value[row].to_bits())
                });
                f32::from_bits(numeric::float(
                    1,
                    numeric::float(1, products[0], products[1]),
                    numeric::float(1, products[2], products[3]),
                ))
            })
        }
        _ => unreachable!(),
    };
    write(vm, target, output)?;
    Ok(NativeReturn::Continue)
}
