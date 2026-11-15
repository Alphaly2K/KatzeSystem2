use crate::Result;
use crate::kcs::{HostState, Interpreter, NativeReturn, matrix};

pub(super) fn call(
    host: &mut HostState,
    index: u32,
    arguments: &[u8],
    vm: &mut Interpreter,
) -> Result<NativeReturn> {
    let args = super::words(arguments, if index == 207 { 1 } else { 4 }, vm.pc)?;
    let mapped = vm.read(args[0], 1).is_ok();
    let identity = || std::array::from_fn(|i| if i % 5 == 0 { 1.0 } else { 0.0 });
    let result = if index == 207 {
        identity()
    } else {
        let mut left = if mapped {
            let bytes = vm.read(args[0], 64)?;
            std::array::from_fn(|i| f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap()))
        } else {
            host.implicit_matrix.map(f32::from_bits)
        };
        if index == 204 {
            for (axis, angle) in args[1..].iter().enumerate() {
                let (sine, cosine) = matrix::sin_cos(f32::from_bits(*angle));
                let mut right = identity();
                match axis {
                    0 => {
                        right[5] = cosine;
                        right[6] = sine;
                        right[9] = -sine;
                        right[10] = cosine;
                    }
                    1 => {
                        right[0] = cosine;
                        right[2] = -sine;
                        right[8] = sine;
                        right[10] = cosine;
                    }
                    _ => {
                        right[0] = cosine;
                        right[1] = sine;
                        right[4] = -sine;
                        right[5] = cosine;
                    }
                }
                left = matrix::multiply(left, right);
            }
        } else {
            let mut right = identity();
            for i in 0..3 {
                right[if index == 205 { 12 + i } else { i * 5 }] = f32::from_bits(args[i + 1]);
            }
            left = matrix::multiply(left, right);
        }
        left
    };
    if mapped {
        let bytes: Vec<_> = result
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        vm.write(args[0], &bytes)?;
    } else {
        host.implicit_matrix = result.map(f32::to_bits);
    }
    Ok(NativeReturn::Continue)
}
