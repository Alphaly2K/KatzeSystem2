use crate::{Error, Result};

/// 图像与普通语言接口共用的前缀和选择。前缀由宿主转为 CP932 字节。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LanguageState {
    pub prefixes: Vec<Vec<u8>>,
    pub default_index: u32,
    pub primary_index: u32,
    pub message_index: u32,
}

impl LanguageState {
    /// 读取启动 XML 的 `language` 段。
    ///
    /// 还原默认编号和 `L%02d` 声明的有序前缀表，首个缺失 `<prefix>` 的
    /// 块结束扫描；主语言和消息语言的初始选择均取默认编号。`<code>`、
    /// `<lang>`、`<accel>`、`<enable_sub>` 与 `<OPTTABLE>` 不进入前缀
    /// 选择，平台语言识别和运行时切换继续由调用方处理。
    pub fn from_xml(input: &[u8]) -> Result<Self> {
        let xml = crate::resource::XmlDocument::parse(input)?;
        let default_index = xml.integer(b"language/default", 0)?;
        let mut prefixes = Vec::new();
        for index in 0..crate::read::MAX_ITEMS {
            let path = format!("language/L{index:02}/prefix");
            let Some(prefix) = xml.text(path.as_bytes())? else {
                break;
            };
            prefixes.push(prefix.to_vec());
        }
        Ok(Self {
            prefixes,
            default_index,
            primary_index: default_index,
            message_index: default_index,
        })
    }

    /// mode 零选择主语言，一选择消息语言；选择越界时回退到默认编号。
    pub fn prefix(&self, mode: u32) -> Option<&[u8]> {
        let index = match mode {
            0 => self.primary_index,
            1 => self.message_index,
            _ => return None,
        };
        self.prefixes
            .get(index as usize)
            .or_else(|| self.prefixes.get(self.default_index as usize))
            .map(|prefix| terminated(prefix))
    }

    /// mode 零替换 %LANG%，一替换 %LANG_M%，其他值保留输入。
    /// 选择越界时使用默认编号，默认也越界时保留输入。
    pub fn expand(&self, input: &[u8], mode: u32) -> Result<Vec<u8>> {
        let input = terminated(input);
        let marker = match mode {
            0 => b"%LANG%".as_slice(),
            1 => b"%LANG_M%".as_slice(),
            _ => return Ok(input.to_vec()),
        };
        let Some(prefix) = self.prefix(mode) else {
            return Ok(input.to_vec());
        };
        if prefix.len() >= 256 {
            return Err(Error::new(0, "语言前缀超过基线容量"));
        }
        let mut output = Vec::new();
        let mut position = 0;
        while position < input.len() {
            let bytes = if input[position..].starts_with(marker) {
                position += marker.len();
                prefix
            } else {
                position += 1;
                &input[position - 1..position]
            };
            if output.len() + bytes.len() >= 2048 {
                return Err(Error::new(0, "语言替换结果超过基线容量"));
            }
            output.extend_from_slice(bytes);
        }
        Ok(output)
    }

    /// IMAGE／BUTTON 的文件名先展开主语言，再展开消息语言。
    pub fn expand_image(&self, input: &[u8]) -> Result<Vec<u8>> {
        self.expand(&self.expand(input, 0)?, 1)
    }
}

fn terminated(bytes: &[u8]) -> &[u8] {
    &bytes[..bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len())]
}
