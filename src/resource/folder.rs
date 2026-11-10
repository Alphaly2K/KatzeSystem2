use crate::{Error, Result};

/// 宿主提供的目录展开上下文，字段均为首零结束的 UTF-16。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FolderPaths {
    /// `%USERPROFILE%` 使用用户应用数据目录。
    pub user_data: Vec<u16>,
    /// `%ProgramData%` 与 `%ALLUSERSPROFILE%` 使用公共应用数据目录。
    pub common_data: Vec<u16>,
    pub brand: Vec<u16>,
    pub title: Vec<u16>,
    pub save: Vec<u16>,
    pub imglib: Vec<u16>,
}

impl FolderPaths {
    /// 一次扫描并按大小写展开已知标记；未知或未闭合标记保持字面值。
    /// 不递归展开替换结果，不改写分隔符，输出至多 259 个单元。
    pub fn expand(&self, source: &[u16]) -> Result<Vec<u16>> {
        let source = terminated(source);
        let fields = [
            ("%USERPROFILE%", &self.user_data),
            ("%ProgramData%", &self.common_data),
            ("%ALLUSERSPROFILE%", &self.common_data),
            ("%brand%", &self.brand),
            ("%title%", &self.title),
            ("%save%", &self.save),
            ("%imglib%", &self.imglib),
        ];
        let mut output = Vec::new();
        let mut cursor = 0;
        while cursor < source.len() {
            let end = if source[cursor] == u16::from(b'%') {
                source[cursor + 1..]
                    .iter()
                    .position(|&unit| unit == u16::from(b'%'))
                    .map_or(source.len(), |length| cursor + length + 2)
            } else {
                cursor + 1
            };
            let token = &source[cursor..end];
            if token.len() >= 260 {
                return Err(Error::new(0, "目录标记超出转换缓冲范围"));
            }
            let value = fields
                .iter()
                .find(|(name, _)| token.iter().copied().eq(name.bytes().map(u16::from)))
                .map_or(token, |(_, value)| terminated(value));
            if value.len() > 259 - output.len() {
                return Err(Error::new(0, "目录展开超出字段容量"));
            }
            output.extend_from_slice(value);
            cursor = end;
        }
        Ok(output)
    }
}

fn terminated(value: &[u16]) -> &[u16] {
    &value[..value
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(value.len())]
}
