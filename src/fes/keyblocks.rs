use super::{Interpreter, resources::integer_prefix};
use crate::{Error, Result, expression::State, read};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyBlock {
    pub size: [i32; 2],
    /// 顺序为上、下、左、右；−1 表示没有相邻块。
    pub neighbors: [i32; 4],
}

impl Default for KeyBlock {
    fn default() -> Self {
        Self {
            size: [0; 2],
            neighbors: [-1; 4],
        }
    }
}

impl Interpreter {
    /// 解码第一段 KEYBLOCK，由宿主在处理初始化声明时调用一次。
    pub fn keyblocks(&mut self, state: &mut State) -> Result<Vec<KeyBlock>> {
        let Some(declaration) = self
            .program
            .declarations
            .iter()
            .find(|d| d.header.split(u8::is_ascii_whitespace).next() == Some(b"KEYBLOCK"))
            .cloned()
        else {
            return Ok(Vec::new());
        };
        let header = self.parameters(&declaration.header, state)?;
        let count = header
            .get(1)
            .and_then(|value| integer_prefix(value))
            .ok_or_else(|| Error::new(0, "KEYBLOCK 数量需要整数前缀"))?;
        if count <= 0 {
            return Ok(Vec::new());
        }
        if count as usize > read::MAX_ITEMS {
            return Err(Error::new(0, "KEYBLOCK 数量超过读取上限"));
        }
        let mut blocks = vec![KeyBlock::default(); count as usize];
        for line in declaration.lines {
            let result = (|| {
                let parameters = self.parameters(&line.bytes, state)?;
                let index = parameters
                    .first()
                    .ok_or_else(|| Error::new(0, "KEYBLOCK 行缺少编号"))?;
                let index = self.component(index, 0, state)?.unwrap_or(0);
                let Some(block) = usize::try_from(index)
                    .ok()
                    .and_then(|index| blocks.get_mut(index))
                else {
                    return Ok(());
                };
                let size = parameters
                    .get(1)
                    .ok_or_else(|| Error::new(0, "KEYBLOCK 行缺少排列尺寸"))?;
                for (i, value) in block.size.iter_mut().enumerate() {
                    *value = self
                        .component(size, i, state)?
                        .ok_or_else(|| Error::new(0, "KEYBLOCK 排列尺寸需要两个分量"))?;
                }
                if let Some(neighbors) = parameters.get(2) {
                    for (i, value) in block.neighbors.iter_mut().enumerate() {
                        if let Some(number) = self.component(neighbors, i, state)? {
                            *value = number;
                        }
                    }
                }
                Ok::<_, Error>(())
            })();
            result.map_err(|error| Error::new(line.offset + error.offset, error.message))?;
        }
        Ok(blocks)
    }
}
