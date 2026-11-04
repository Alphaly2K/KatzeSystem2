use super::{
    Event, Host, Interpreter, NativeReturn, Program, ScriptRandom, matrix, numeric, strings,
};
use crate::{Error, Result, Step, read};

const EXTERNAL: u32 = 0x8000_0000;
const EXTERNAL_SIZE: usize = 0x4a8;

/// 运动 VM 的对象属性位模式及外部控制数据；属性块由对象宿主提供。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MotionState {
    pub properties: [u32; 36],
    pub elapsed: u32,
    pub running: i32,
    pub finishing: i32,
    pub parameters: [u32; 256],
}

impl Default for MotionState {
    fn default() -> Self {
        Self {
            properties: [0; 36],
            elapsed: 0,
            running: 1,
            finishing: 0,
            parameters: [0; 256],
        }
    }
}

impl MotionState {
    pub fn with_parameters(parameters: &[f32]) -> Result<Self> {
        if parameters.len() > 256 {
            return Err(Error::new(0, "运动参数超过上限"));
        }
        let mut state = Self::default();
        for (target, value) in state.parameters.iter_mut().zip(parameters) {
            *target = value.to_bits();
        }
        Ok(state)
    }

    pub fn external_data(&self) -> Vec<u8> {
        let mut bytes: Vec<_> = self
            .properties
            .iter()
            .copied()
            .chain([self.elapsed, self.running as u32, self.finishing as u32])
            .chain(self.parameters.iter().copied())
            .flat_map(u32::to_le_bytes)
            .collect();
        bytes.resize(EXTERNAL_SIZE, 0);
        bytes
    }

    pub fn read(vm: &Interpreter) -> Result<Self> {
        let bytes = vm.read(EXTERNAL, EXTERNAL_SIZE)?;
        Ok(Self {
            properties: std::array::from_fn(|i| read::u32_at(bytes, i * 4).unwrap()),
            elapsed: read::u32_at(bytes, 144)?,
            running: read::u32_at(bytes, 148)? as i32,
            finishing: read::u32_at(bytes, 152)? as i32,
            parameters: std::array::from_fn(|i| read::u32_at(bytes, 156 + i * 4).unwrap()),
        })
    }
}

enum Invocation {
    Update,
    Finish([u8; 12]),
}

/// 运动入口的时间控制；预算让出会保留本次调用，下一次相同方法继续执行。
pub struct MotionTask {
    pub interpreter: Interpreter,
    pub base_time: u32,
    /// 装载方提供的 UTF-16 运动名称；快照保留首个零之前的内容。
    pub snapshot_name: Vec<u16>,
    /// 快照中的原始控制 DWORD，与 running／finishing 分别保存。
    pub snapshot_flags: u32,
    /// 登记的保存数据地址及字节数；地址零表示没有保存数据。
    pub snapshot_data: Option<(u32, usize)>,
    invocation: Option<Invocation>,
}

impl MotionTask {
    pub fn new(program: Program, state: MotionState, base_time: u32) -> Result<Self> {
        Ok(Self {
            interpreter: Interpreter::new(program, state.external_data())?,
            base_time,
            snapshot_name: Vec::new(),
            snapshot_flags: 0,
            snapshot_data: None,
            invocation: None,
        })
    }

    pub fn state(&self) -> Result<MotionState> {
        MotionState::read(&self.interpreter)
    }

    pub fn set_properties(&mut self, properties: &[u32; 36]) -> Result<()> {
        let bytes: Vec<_> = properties.iter().flat_map(|n| n.to_le_bytes()).collect();
        self.interpreter.write(EXTERNAL, &bytes)
    }

    /// 未到开始时间返回 None；同一次预算让出期间固定 elapsed。
    pub fn update(
        &mut self,
        now: u32,
        host: &mut impl Host,
        budget: usize,
    ) -> Result<Option<Step<Event>>> {
        if matches!(self.invocation, Some(Invocation::Finish(_))) {
            return Err(Error::new(self.interpreter.pc, "运动结束调用尚未完成"));
        }
        if self.invocation.is_none() {
            if word(&self.interpreter, EXTERNAL + 152)? == 0 && self.base_time > now {
                return Ok(None);
            }
            self.interpreter.write(
                EXTERNAL + 144,
                &now.wrapping_sub(self.base_time).to_le_bytes(),
            )?;
            self.interpreter.reset_execution();
            self.invocation = Some(Invocation::Update);
        }
        self.run(host, budget).map(Some)
    }

    /// 用结束上下文重新执行入口，完成后恢复时间和两个控制标志。
    pub fn finish(&mut self, host: &mut impl Host, budget: usize) -> Result<Step<Event>> {
        if !matches!(self.invocation, Some(Invocation::Finish(_))) {
            let saved = self
                .interpreter
                .read(EXTERNAL + 144, 12)?
                .try_into()
                .unwrap();
            self.interpreter
                .write(EXTERNAL + 144, &u32::MAX.to_le_bytes())?;
            self.interpreter
                .write(EXTERNAL + 152, &1u32.to_le_bytes())?;
            self.interpreter.reset_execution();
            self.invocation = Some(Invocation::Finish(saved));
        }
        self.run(host, budget)
    }

    fn run(&mut self, host: &mut impl Host, budget: usize) -> Result<Step<Event>> {
        let result = self.interpreter.run(host, budget);
        if !matches!(result, Ok(Step::Yield)) {
            if let Some(Invocation::Finish(saved)) = self.invocation.take() {
                self.interpreter.write(EXTERNAL + 144, &saved)?;
            }
            self.invocation = None;
        }
        result
    }
}

#[derive(Default)]
pub struct MotionHost {
    pub random: ScriptRandom,
    pub debug_messages: Vec<Vec<u8>>,
}

impl Host for MotionHost {
    fn call(&mut self, index: u32, arguments: &[u8], vm: &mut Interpreter) -> Result<NativeReturn> {
        let count = match index {
            38..=40 => 0,
            0..=6 | 8..=10 | 12 | 14..=17 | 26 | 27 | 31 | 34 | 35 => 1,
            7 | 11 | 13 | 28..=30 | 32 | 33 | 36 | 37 | 42 => 2,
            44 => 3,
            18..=20 | 23..=25 | 43 => 4,
            21 | 22 => 6,
            41 => 5,
            _ => {
                return Err(Error::new(
                    vm.pc,
                    format!("未支持的运动 KCS native {index}"),
                ));
            }
        };
        if arguments.len() != count * 4 {
            return Err(Error::new(vm.pc, "运动 KCS native 参数字节数不匹配"));
        }
        let a: Vec<_> = arguments
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        let result = match index {
            0 => {
                self.debug_messages.push(strings::format(vm, a[0], 0)?);
                return Ok(NativeReturn::Continue);
            }
            1..=13 => numeric::native_math(index + 5, &a),
            14 => (f32::from_bits(a[0]) * std::f32::consts::PI / 180.0).to_bits(),
            15 => (f32::from_bits(a[0]) * 180.0 / std::f32::consts::PI).to_bits(),
            16 => (a[0] as i32).wrapping_abs() as u32,
            17 => numeric::convert(0x39, (f32::from_bits(a[0]) + 0.5).floor().to_bits()),
            18..=22 => {
                if index <= 20 {
                    let v = [word(vm, a[0])?, word(vm, a[1])?, word(vm, a[2])?].map(f32::from_bits);
                    let s = f32::from_bits(numeric::native_math(6, &a[3..4]));
                    let c = f32::from_bits(numeric::native_math(7, &a[3..4]));
                    let [x, y, z] = v;
                    let v = match index {
                        18 => [x, z * s + y * c, z * c - y * s],
                        19 => [x * c - z * s, y, z * c + x * s],
                        20 => [x * c + y * s, y * c - x * s, z],
                        _ => unreachable!(),
                    };
                    for (address, value) in a.iter().zip(v) {
                        vm.write(*address, &value.to_le_bytes())?;
                    }
                } else {
                    for i in 0..3 {
                        let value = numeric::float(
                            if index == 21 { 1 } else { 3 },
                            a[i + 3],
                            word(vm, a[i])?,
                        );
                        vm.write(a[i], &value.to_le_bytes())?;
                    }
                }
                return Ok(NativeReturn::Continue);
            }
            23..=30 | 37 | 43 | 44 => {
                matrix_operation(index, &a, vm)?;
                return Ok(NativeReturn::Continue);
            }
            31 => {
                let v = vector::<4>(vm, a[0])?;
                write_vector(vm, a[0], matrix::normalize4(v))?;
                return Ok(NativeReturn::Continue);
            }
            32 | 33 => {
                for i in 0..4 {
                    let offset = i * 4;
                    let left = word(vm, a[0].wrapping_add(offset))?;
                    let right = word(vm, a[1].wrapping_add(offset))?;
                    let (left, right) = if index == 32 && i > 0 {
                        (right, left)
                    } else {
                        (left, right)
                    };
                    let value = numeric::float(if index == 32 { 1 } else { 2 }, left, right);
                    vm.write(a[0].wrapping_add(offset), &value.to_le_bytes())?;
                }
                return Ok(NativeReturn::Continue);
            }
            34 | 35 => {
                let v = vector::<4>(vm, a[0])?;
                let square = ((v[0] * v[0] + v[1] * v[1]) + v[2] * v[2]) + v[3] * v[3];
                if index == 34 {
                    numeric::native_math(17, &[square.to_bits()])
                } else {
                    square.to_bits()
                }
            }
            36 => {
                let scale = f32::from_bits(word(vm, a[1])?);
                let v = vector::<4>(vm, a[0])?;
                write_vector(vm, a[0], v.map(|n| n * scale))?;
                return Ok(NativeReturn::Continue);
            }
            38 => {
                vm.write(EXTERNAL + 148, &0u32.to_le_bytes())?;
                return Ok(NativeReturn::YieldVoid);
            }
            39 => self.random.next_integer(),
            40 => self.random.next_float().to_bits(),
            41 => {
                let v: Vec<_> = a.iter().map(|&n| f32::from_bits(n)).collect();
                if v[2] >= v[4] {
                    a[0]
                } else if v[4] >= v[3] {
                    a[1]
                } else {
                    (((v[4] - v[2]) / (v[3] - v[2])) * (v[1] - v[0]) + v[0]).to_bits()
                }
            }
            42 => numeric::native_triangle(f32::from_bits(a[0]), f32::from_bits(a[1])).to_bits(),
            _ => unreachable!(),
        };
        Ok(NativeReturn::Value(result))
    }
}

fn word(vm: &Interpreter, address: u32) -> Result<u32> {
    Ok(u32::from_le_bytes(vm.read(address, 4)?.try_into().unwrap()))
}
fn vector<const N: usize>(vm: &Interpreter, address: u32) -> Result<[f32; N]> {
    let bytes = vm.read(address, N * 4)?;
    Ok(std::array::from_fn(|i| {
        f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
    }))
}
fn write_vector<const N: usize>(vm: &mut Interpreter, address: u32, v: [f32; N]) -> Result<()> {
    let bytes: Vec<_> = v.into_iter().flat_map(f32::to_le_bytes).collect();
    vm.write(address, &bytes)
}
fn identity() -> [f32; 16] {
    std::array::from_fn(|i| if i % 5 == 0 { 1.0 } else { 0.0 })
}
fn matrix_operation(index: u32, a: &[u32], vm: &mut Interpreter) -> Result<()> {
    if index == 26 {
        return write_vector(vm, a[0], identity());
    }
    if index == 27 {
        if let Some(value) = matrix::inverse(vector::<16>(vm, a[0])?) {
            write_vector(vm, a[0], value)?;
        }
        return Ok(());
    }
    if index == 28 {
        for offset in [0, 16, 32, 48] {
            let bytes = vm.read(a[1].wrapping_add(offset), 16)?.to_vec();
            vm.write(a[0].wrapping_add(offset), &bytes)?;
        }
        return Ok(());
    }
    if index == 30 {
        for offset in (0..64).step_by(4) {
            let value = numeric::float(
                2,
                word(vm, a[0].wrapping_add(offset))?,
                word(vm, a[1].wrapping_add(offset))?,
            );
            vm.write(a[0].wrapping_add(offset), &value.to_le_bytes())?;
        }
        return Ok(());
    }
    if index == 37 {
        let v = vector::<4>(vm, a[0])?;
        let matrix = vector::<16>(vm, a[1])?;
        let out = matrix::transform(v, matrix);
        return write_vector(vm, a[0], out);
    }
    let mut left = vector::<16>(vm, a[0])?;
    if index == 44 {
        let reference = vector::<16>(vm, a[1])?;
        let angles = vector::<3>(vm, a[2])?;
        let transform = |v| matrix::transform(v, reference);
        let mut x = transform([-1.0, 0.0, 0.0, 0.0]);
        x[1] = 0.0;
        let x = matrix::normalize4(x);
        let z = matrix::normalize4(transform([0.0, 0.0, -1.0, 0.0]));
        for (axis, angle) in [[x[0], x[1], x[2]], [0.0, -1.0, 0.0], [z[0], z[1], z[2]]]
            .into_iter()
            .zip(angles)
        {
            left = matrix::multiply(left, matrix::rotation_axis(axis, angle));
        }
    } else if index == 43 {
        left = matrix::multiply(
            left,
            matrix::yaw_pitch_roll([a[1], a[2], a[3]].map(f32::from_bits)),
        );
    } else if index == 29 {
        left = matrix::multiply(left, vector::<16>(vm, a[1])?);
    } else if index == 24 || index == 25 {
        let mut right = identity();
        for i in 0..3 {
            right[if index == 24 { 12 + i } else { i * 5 }] = f32::from_bits(a[i + 1]);
        }
        left = matrix::multiply(left, right);
    } else {
        for axis in 0..3 {
            let s = f32::from_bits(numeric::native_math(6, &a[axis + 1..axis + 2]));
            let c = f32::from_bits(numeric::native_math(7, &a[axis + 1..axis + 2]));
            let mut right = identity();
            match axis {
                0 => {
                    right[5] = c;
                    right[6] = s;
                    right[9] = -s;
                    right[10] = c;
                }
                1 => {
                    right[0] = c;
                    right[2] = -s;
                    right[8] = s;
                    right[10] = c;
                }
                2 => {
                    right[0] = c;
                    right[1] = s;
                    right[4] = -s;
                    right[5] = c;
                }
                _ => unreachable!(),
            };
            left = matrix::multiply(left, right);
        }
    }
    write_vector(vm, a[0], left)
}
