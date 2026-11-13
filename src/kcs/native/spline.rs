use crate::kcs::{Interpreter, NativeReturn, numeric};
use crate::{Error, Result, read};

fn f(op: u8, a: u32, b: u32) -> u32 {
    numeric::float_ieee(op, a, b)
}

pub(super) fn prepare(args: &[u32], vm: &mut Interpreter) -> Result<NativeReturn> {
    let count = args[1];
    if (count.wrapping_sub(1) as i32) < 2 {
        return Ok(NativeReturn::Continue);
    }
    let count = count as usize;
    let length = count
        .checked_mul(20)
        .filter(|&n| n <= read::MAX_BYTES)
        .ok_or_else(|| Error::new(vm.pc, "样条控制点超过存储上限"))?;
    let mut points: Vec<[u32; 5]> = vm
        .read(args[0], length)?
        .chunks_exact(20)
        .map(|p| {
            std::array::from_fn(|i| u32::from_le_bytes(p[i * 4..i * 4 + 4].try_into().unwrap()))
        })
        .collect();
    let n = count - 1;
    points[0][4] = f(2, points[1][0], points[0][0]);
    points[1][3] = f(4, f(2, points[1][1], points[0][1]), points[0][4]);
    for i in 1..n {
        let interval = f(2, points[i + 1][0], points[i][0]);
        points[i][4] = interval;
        let diagonal = f(1, points[i - 1][4], interval);
        points[i][2] = f(1, diagonal, diagonal);
        points[i + 1][3] = f(4, f(2, points[i + 1][1], points[i][1]), interval);
        points[i][3] = f(2, points[i + 1][3], points[i][3]);
    }
    points[0][2] = points[0][4] ^ 0x8000_0000;
    points[n][2] = points[n - 1][4] ^ 0x8000_0000;
    if count == 3 {
        points[0][3] = 0;
        points[n][3] = 0;
    } else {
        points[0][3] = f(
            2,
            f(4, points[2][3], f(2, points[3][0], points[1][0])),
            f(4, points[1][3], f(2, points[2][0], points[0][0])),
        );
        points[n][3] = f(
            2,
            f(4, points[n - 1][3], f(2, points[n][0], points[n - 2][0])),
            f(
                4,
                points[n - 2][3],
                f(2, points[n - 1][0], points[n - 3][0]),
            ),
        );
        let square = numeric::native_math(18, &[points[0][4], 2f32.to_bits()]);
        points[0][3] = f(
            4,
            f(3, square, points[0][3]),
            f(2, points[3][0], points[1][0]),
        );
        let square = numeric::native_math(18, &[points[n - 1][4], 2f32.to_bits()]);
        points[n][3] = f(
            4,
            f(3, square, points[n][3]),
            f(2, points[n][0], points[n - 3][0]),
        ) ^ 0x8000_0000;
    }
    // 在原控制点记录内消元；每次运算按单精度舍入，保留边界条件的独立顺序。
    for i in 1..=n {
        let ratio = f(4, points[i - 1][4], points[i - 1][2]);
        points[i][2] = f(2, points[i][2], f(3, points[i - 1][4], ratio));
        points[i][3] = f(2, points[i][3], f(3, points[i - 1][3], ratio));
    }
    points[n][3] = f(4, points[n][3], points[n][2]);
    for i in (0..n).rev() {
        points[i][3] = f(
            4,
            f(2, points[i][3], f(3, points[i + 1][3], points[i][4])),
            points[i][2],
        );
    }
    points[n][2] = f(
        1,
        f(
            3,
            f(1, f(1, points[n][3], points[n][3]), points[n - 1][3]),
            points[n - 1][4],
        ),
        f(4, f(2, points[n][1], points[n - 1][1]), points[n - 1][4]),
    );
    for i in 0..n {
        let current = points[i][3];
        let next = points[i + 1][3];
        let interval = points[i][4];
        let slope = f(4, f(2, points[i + 1][1], points[i][1]), interval);
        points[i][2] = f(2, slope, f(3, f(1, f(1, current, current), next), interval));
        points[i][4] = f(4, f(2, next, current), interval);
        points[i][3] = f(3, current, 3f32.to_bits());
    }
    points[n][4] = points[n - 1][4];
    let bytes: Vec<_> = points
        .into_iter()
        .flatten()
        .flat_map(u32::to_le_bytes)
        .collect();
    vm.write(args[0], &bytes)?;
    Ok(NativeReturn::Continue)
}
