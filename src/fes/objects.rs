use super::{Event, Interpreter, Line};
use crate::{Error, Result, expression::State, read, text};

const KINDS: &[&str] = &[
    "BASE",
    "CIMAGE",
    "IMAGE",
    "CPLANE",
    "PLANE",
    "VIEW",
    "BUTTON",
    "DRAW",
    "WIPE",
    "SOUND",
    "SNDTABLE",
    "PANEL",
    "SAVEPANEL",
    "SCROLL",
    "SCROLL2",
    "SLIDER",
    "BGTABLE",
    "BGTABLE2",
    "CHTABLE",
    "CHTABLE2",
    "CHTABLE3",
    "STRING",
    "IMGWND",
    "PARTICLE",
    "LOGSTRING",
    "DIFIMAGE",
    "RDRAW",
    "RWIPE",
    "NUMIMG",
    "NVSTRING",
    "WBREAK",
    "RWMOS",
    "RFMOS",
    "M2PLANE",
    "STRTABLE",
    "FILELIST",
    "EFKPLANE",
    "STRING8",
    "LOGSTRING8",
    "NVSTRING8",
    "VIEWPORT",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectDefinition {
    pub kind: &'static str,
    pub name: String,
    pub banks: u32,
}

impl Interpreter {
    pub(super) fn define_object(&mut self, line: &Line, state: &mut State) -> Result<Event> {
        let expanded = text::expand(&line.bytes, &state.strings)?;
        let parameters = self.parameters(&expanded, state)?;
        let kind = parameters
            .first()
            .and_then(|name| {
                KINDS
                    .iter()
                    .find(|kind| kind.as_bytes().eq_ignore_ascii_case(name))
            })
            .copied()
            .ok_or_else(|| Error::new(line.offset, "未知 FES 对象类型"))?;
        let target = parameters
            .get(1)
            .ok_or_else(|| Error::new(line.offset, "FES 对象声明缺少名称"))?;
        let split = target
            .iter()
            .position(|&b| matches!(b, b'[' | b' '))
            .unwrap_or(target.len());
        let name = text::ascii(&target[..split])?;
        if name.is_empty() {
            return Err(Error::new(line.offset, "FES 对象名称为空"));
        }
        let count = if target.get(split) == Some(&b'[') {
            let start = split + 1;
            let end = target[start..]
                .iter()
                .position(|&b| matches!(b, b']' | b' '))
                .map(|n| start + n)
                .ok_or_else(|| Error::new(line.offset, "FES 对象数量缺少右方括号"))?;
            if end == start || target[end] != b']' {
                return Err(Error::new(line.offset, "FES 对象数量表达式无效"));
            }
            self.eval(&target[start..end], state)?
        } else {
            -1
        };
        if count == -1 && self.object_banks.get(name).is_some_and(|&n| n > 1) {
            return Err(Error::new(line.offset, "重定义 FES 对象数组需要明确数量"));
        }
        let banks = if count == -1 || count == 0 { 1 } else { count };
        if banks < 0 || banks as usize > read::MAX_ITEMS {
            return Err(Error::new(line.offset, "FES 对象数量超出上限"));
        }
        let definition = ObjectDefinition {
            kind,
            name: name.to_owned(),
            banks: banks as u32,
        };
        self.object_banks
            .insert(definition.name.clone(), definition.banks);
        Ok(Event::CreateObject(definition))
    }
}
