use super::{LanguageState, LanguageTarget, StringTables};
use crate::{Error, Result, resource::XmlDocument};

/// 平台字符串表的宽字符路径配置；与脚本接口使用的字节前缀分别保存。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StringTableConfig {
    pub prefixes: Vec<Vec<u16>>,
    pub game_path: Vec<u16>,
}

impl StringTableConfig {
    pub fn from_xml(input: &[u8]) -> Result<Self> {
        let xml = XmlDocument::parse(input)?;
        let mut prefixes = Vec::new();
        for index in 0..crate::read::MAX_ITEMS {
            let path = format!("language/L{index:02}/prefix");
            let Some(prefix) = xml.wide_text(path.as_bytes())? else {
                break;
            };
            prefixes.push(prefix);
        }
        Ok(Self {
            prefixes,
            game_path: xml.wide_text(b"language/OPTTABLE")?.unwrap_or_default(),
        })
    }

    /// 界面请求仅在当前界面编号有效时重载两张表；主文字请求始终重载游戏表，
    /// 当前主文字编号有效时按平台消息处理顺序再重载一次。
    /// 副文字请求不读取字符串表。读取时使用当前选择，缺失文件清除旧表。
    pub fn reload(
        &self,
        target: LanguageTarget,
        languages: &LanguageState,
        tables: &mut StringTables,
        mut read: impl FnMut(&[u16]) -> Result<Option<Vec<u8>>>,
    ) -> Result<()> {
        match target {
            LanguageTarget::Interface => {
                let Some(prefix) = self.prefixes.get(languages.primary_index as usize) else {
                    return Ok(());
                };
                let path = [wide("language_"), terminated(prefix).to_vec(), wide(".txt")].concat();
                if path.len() >= 1024 {
                    return Err(Error::new(0, "界面字符串表路径超过基线容量"));
                }
                tables.reload(0, || read(&path))?;
            }
            LanguageTarget::MainText => {}
            LanguageTarget::Subtitle => return Ok(()),
        }
        let path = self.expand(
            &self.game_path,
            languages.primary_index,
            languages.default_index,
            "%LANG%",
        )?;
        let path = self.expand(
            &path,
            languages.message_index,
            languages.default_index,
            "%LANG_M%",
        )?;
        let count = if target == LanguageTarget::MainText
            && self
                .prefixes
                .get(languages.message_index as usize)
                .is_some()
        {
            2
        } else {
            1
        };
        for _ in 0..count {
            tables.reload(1, || read(&path))?;
        }
        Ok(())
    }

    fn expand(&self, input: &[u16], index: u32, default: u32, marker: &str) -> Result<Vec<u16>> {
        let input = terminated(input);
        check_path(input)?;
        let Some(prefix) = self
            .prefixes
            .get(index as usize)
            .or_else(|| self.prefixes.get(default as usize))
        else {
            return Ok(input.to_vec());
        };
        let prefix = terminated(prefix);
        let marker = wide(marker);
        let mut output = Vec::new();
        let mut position = 0;
        while position < input.len() {
            if input[position..].starts_with(&marker) {
                output.extend_from_slice(prefix);
                position += marker.len();
            } else {
                output.push(input[position]);
                position += 1;
            }
            check_path(&output)?;
        }
        Ok(output)
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn terminated(input: &[u16]) -> &[u16] {
    &input[..input
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(input.len())]
}

fn check_path(path: &[u16]) -> Result<()> {
    if path.len() >= 2048 {
        return Err(Error::new(0, "字符串表路径超过基线容量"));
    }
    Ok(())
}
