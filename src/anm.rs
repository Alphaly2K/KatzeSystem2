use crate::{Error, Result, Step, read};

mod image;
mod timeline;
pub use image::ImageAnimation;
pub use timeline::Timeline;

#[derive(Clone, Copy, Debug, Default)]
pub struct Operand {
    pub kind: u32,
    pub value: u32,
}

#[derive(Clone, Debug)]
pub struct Instruction {
    pub opcode: u32,
    pub operands: [Operand; 8],
}

#[derive(Clone, Debug)]
pub struct Program {
    pub instructions: Vec<Instruction>,
}

impl Program {
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.get(..4) != Some(b"ANM\0") || data.len() > read::MAX_BYTES {
            return Err(Error::new(0, "无效 ANM 容器"));
        }
        let n = read::u32_at(data, 8)? as usize;
        if data.len() < 32 || n == 0 || n > (data.len() - 32) / 68 {
            return Err(Error::new(8, "ANM 指令数量与长度不匹配"));
        }
        let mut instructions = Vec::new();
        for i in 0..n {
            let p = 32 + i * 68;
            let opcode = read::u32_at(data, p)?;
            let mut operands = [Operand::default(); 8];
            for (j, operand) in operands.iter_mut().enumerate() {
                *operand = Operand {
                    kind: read::u32_at(data, p + 4 + j * 8)?,
                    value: read::u32_at(data, p + 8 + j * 8)?,
                };
            }
            instructions.push(Instruction { opcode, operands });
        }
        Ok(Self { instructions })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Image { id: i32, duration: i32 },
    Wait { duration: i32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayState {
    pub image: i32,
    pub blend: i32,
    pub visible: i32,
    pub x: i32,
    pub y: i32,
}

impl Default for DisplayState {
    fn default() -> Self {
        Self {
            image: -1,
            blend: 255,
            visible: 1,
            x: 0,
            y: 0,
        }
    }
}

impl DisplayState {
    /// 将动画显示值与图像自身属性组合；保留帧 ID 和原始显隐值。
    pub fn compose(self, position: [f32; 2], blend: i32, visible: i32) -> Self {
        let position = position.map(|value| {
            if (-2147483648.0..2147483648.0).contains(&value) {
                value as i32
            } else {
                i32::MIN
            }
        });
        let product = (self.blend as u32).wrapping_mul(blend as u32);
        Self {
            image: self.image,
            blend: if product < 0xff00 {
                (product / 255) as i32
            } else {
                255
            },
            visible: if visible == 0 { 0 } else { self.visible },
            x: position[0].wrapping_add(self.x),
            y: position[1].wrapping_add(self.y),
        }
    }
}

/// 宿主在完成事件给出的持续时间后再次调用 run。
pub struct Interpreter {
    pub pc: usize,
    pub variables: [i32; 64],
    pub display: DisplayState,
    pub max_image_id: i32,
    program: Program,
}

impl Interpreter {
    pub fn new(program: Program) -> Self {
        Self {
            program,
            pc: 0,
            variables: [0; 64],
            display: DisplayState::default(),
            max_image_id: 0,
        }
    }
    fn value(&self, op: Operand) -> Result<i32> {
        match op.kind {
            0 | 2 => Ok(op.value as i32),
            1 => self
                .variables
                .get(op.value as usize)
                .copied()
                .ok_or_else(|| Error::new(self.pc, "ANM 变量编号越界")),
            _ => Err(Error::new(self.pc, "未支持的 ANM 参数类型")),
        }
    }
    fn jump(&mut self, target: i32) -> Result<()> {
        if target < 0 || target as usize > self.program.instructions.len() {
            return Err(Error::new(self.pc, "ANM 跳转目标越界"));
        }
        self.pc = target as usize;
        Ok(())
    }
    pub fn run(&mut self, budget: usize) -> Result<Step<Event>> {
        self.advance(budget, None)
    }
    /// 随机源由宿主统一提供，所有动画可共享同一条随机序列。
    pub fn run_with_random(
        &mut self,
        budget: usize,
        random: &mut impl FnMut() -> u32,
    ) -> Result<Step<Event>> {
        self.advance(budget, Some(random))
    }
    fn interval(
        &self,
        low: Operand,
        high: Operand,
        random: &mut Option<&mut dyn FnMut() -> u32>,
    ) -> Result<i32> {
        let low = self.value(low)? as u32;
        let span = (self.value(high)? as u32).wrapping_sub(low);
        if span == 0 {
            return Ok(low as i32);
        }
        let random = random
            .as_mut()
            .ok_or_else(|| Error::new(self.pc, "ANM 随机区间需要宿主随机源"))?;
        Ok(low.wrapping_add(random() % span) as i32)
    }
    fn advance(
        &mut self,
        budget: usize,
        mut random: Option<&mut dyn FnMut() -> u32>,
    ) -> Result<Step<Event>> {
        for _ in 0..budget {
            let Some(ins) = self.program.instructions.get(self.pc) else {
                return Ok(Step::Halted);
            };
            let op = ins.opcode;
            let args = ins.operands;
            let a = self.value(args[0])?;
            match op {
                0 | 15 => {
                    let duration = if op == 0 {
                        self.interval(args[1], args[2], &mut random)?
                    } else {
                        self.interval(args[0], args[1], &mut random)?
                    };
                    self.pc += 1;
                    return Ok(Step::Event(if op == 0 {
                        self.display.image = a;
                        Event::Image { id: a, duration }
                    } else {
                        Event::Wait { duration }
                    }));
                }
                3 => {
                    self.jump(a)?;
                    continue;
                }
                1 | 2 | 11 | 16 | 17 => {
                    let index = usize::try_from(a)
                        .ok()
                        .filter(|&i| i < 64)
                        .ok_or_else(|| Error::new(self.pc, "ANM 变量编号越界"))?;
                    let n = self.variables[index];
                    match op {
                        1 => {
                            self.variables[index] = self.interval(args[1], args[2], &mut random)?;
                        }
                        11 => self.variables[index] = self.max_image_id,
                        2 => {
                            if n > 0 {
                                self.variables[index] = n.wrapping_sub(1);
                            } else {
                                self.jump(self.value(args[1])?)?;
                                continue;
                            }
                        }
                        16 => self.variables[index] = n.wrapping_add(self.value(args[1])?),
                        17 => self.variables[index] = n.wrapping_sub(self.value(args[1])?),
                        _ => unreachable!(),
                    }
                    self.pc += 1;
                }
                4..=10 => {
                    let b = if op == 4 { 0 } else { self.value(args[1])? };
                    let jump = match op {
                        4 | 7 => a > b,
                        5 => a == b,
                        6 => a != b,
                        8 => a < b,
                        9 => a >= b,
                        10 => a <= b,
                        _ => unreachable!(),
                    };
                    if jump {
                        self.jump(self.value(args[if op == 4 { 1 } else { 2 }])?)?;
                    } else {
                        self.pc += 1;
                    }
                }
                12..=14 => {
                    match op {
                        12 => self.display.blend = a,
                        13 => self.display.visible = a,
                        14 => {
                            self.display.x = a;
                            self.display.y = self.value(args[1])?;
                        }
                        _ => unreachable!(),
                    }
                    self.pc += 1;
                }
                _ => return Err(Error::new(self.pc, format!("未支持的 ANM 指令 {op}"))),
            }
        }
        Ok(Step::Yield)
    }
}
