use super::words;
use crate::Result;
use crate::kcs::{HostState, Interpreter, NativeReturn, numeric};

pub(super) fn call(
    host: &mut HostState,
    index: u32,
    arguments: &[u8],
    vm: &mut Interpreter,
) -> Result<NativeReturn> {
    if index == 33 {
        return Ok(NativeReturn::Value(host.random.next_integer()));
    }
    if index == 34 {
        return Ok(NativeReturn::Value(host.random.next_float().to_bits()));
    }
    let count = match index {
        59 | 61 | 397 => 2,
        218 | 318 => 5,
        _ => 1,
    };
    let args = words(arguments, count, vm.pc)?;
    Ok(match index {
        218 => {
            let source: [i32; 4] = {
                let bytes = vm.read(args[0], 16)?;
                std::array::from_fn(|i| {
                    i32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
                })
            };
            let mut rectangle = [
                source[0].max(args[1] as i32),
                source[1].max(args[2] as i32),
                source[2].min(args[3] as i32),
                source[3].min(args[4] as i32),
            ];
            if rectangle[0] >= rectangle[2] || rectangle[1] >= rectangle[3] {
                rectangle = [0; 4];
            }
            let bytes: Vec<_> = rectangle.into_iter().flat_map(i32::to_le_bytes).collect();
            vm.write(args[0], &bytes)?;
            NativeReturn::Continue
        }
        318 => {
            let [from, to, start, end, now] = args[..5].try_into().unwrap();
            let value = if f32::from_bits(start) >= f32::from_bits(now) {
                from
            } else if f32::from_bits(now) >= f32::from_bits(end) {
                to
            } else {
                let elapsed = numeric::float_ieee(2, now, start);
                let duration = numeric::float_ieee(2, end, start);
                let distance = numeric::float_ieee(2, to, from);
                let fraction = numeric::float_ieee(4, elapsed, duration);
                let scaled = numeric::float_ieee(3, fraction, distance);
                numeric::float_ieee(1, scaled, from)
            };
            NativeReturn::Value(value)
        }
        319 => {
            let shifted = numeric::float_ieee(1, args[0], 0.5f32.to_bits());
            let rounded = numeric::native_math(15, &[shifted]);
            NativeReturn::Value(numeric::convert(0x39, rounded))
        }
        397 => NativeReturn::Value(if f32::from_bits(args[1]).is_nan() {
            args[1] | 0x0040_0000
        } else {
            numeric::native_math(12, &args)
        }),
        50..=61 => {
            let function = match index {
                50..=55 => index - 44,
                56..=61 => index - 43,
                _ => unreachable!(),
            };
            let mut value = numeric::native_math(function, &args);
            // 普通表调用 SSE2 precise pow，奇整数指数会清除 NaN 底数符号。
            let exponent = args.get(1).copied().map(f32::from_bits).unwrap_or(0.0);
            if index == 61
                && f32::from_bits(args[0]).is_nan()
                && exponent.abs() < 16_777_216.0
                && exponent.fract() == 0.0
                && exponent as i32 & 1 != 0
            {
                value &= 0x7fff_ffff;
            }
            NativeReturn::Value(value)
        }
        62 | 63 => {
            let (multiplier, divisor) = if index == 62 {
                (std::f32::consts::PI.to_bits(), 180.0f32.to_bits())
            } else {
                (180.0f32.to_bits(), std::f32::consts::PI.to_bits())
            };
            let product = numeric::float_ieee(3, args[0], multiplier);
            NativeReturn::Value(numeric::float_ieee(4, product, divisor))
        }
        64 => NativeReturn::Value((args[0] as i32).wrapping_abs() as u32),
        _ => unreachable!(),
    })
}
