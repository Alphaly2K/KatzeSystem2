use super::terminated;

/// 声音组的表键、分类编号、原始标签及包含两端的物理通道范围。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SoundGroup {
    /// 新配置与 id 相同；旧配置可以将多个键映射到同一分类。
    pub key: u32,
    pub id: u32,
    pub label: Vec<u8>,
    pub range: [i32; 2],
}

/// 启动宿主提供的声音组，保持配置顺序及重复编号。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SoundGroups {
    pub entries: Vec<SoundGroup>,
}

impl SoundGroups {
    /// 读取启动文档的八个固定声音组槽位，跳过编号为全一的停用项。
    /// 表键保留槽位编号，标签取 type，物理通道范围包含两端。
    pub fn from_startup_xml(bytes: &[u8]) -> crate::Result<Self> {
        let xml = crate::resource::XmlDocument::parse(bytes)?;
        let mut entries = Vec::new();
        for key in 0..8 {
            let path = format!("SOUNDGROUP/group{key:02}");
            let id = xml.integer(format!("{path}/id").as_bytes(), u32::MAX)?;
            let label = xml
                .text(format!("{path}/type").as_bytes())?
                .unwrap_or_default();
            if label.len() >= 16 {
                return Err(crate::Error::new(0, "声音组类别名称超过基线容量"));
            }
            if id != u32::MAX {
                entries.push(SoundGroup {
                    key,
                    id,
                    label: label.to_vec(),
                    range: [
                        xml.integer(format!("{path}/min").as_bytes(), u32::MAX)? as i32,
                        xml.integer(format!("{path}/max").as_bytes(), u32::MAX)? as i32,
                    ],
                });
            }
        }
        Ok(Self { entries })
    }

    /// 新配置按原始标签精确查首项，缺失返回零；名称在首零结束。
    pub fn id(&self, label: &[u8]) -> u32 {
        let label = terminated(label);
        self.entries
            .iter()
            .find(|group| group.label == label)
            .map_or(0, |group| group.id)
    }

    /// 旧配置的固定组名忽略 ASCII 大小写，未识别名称使用五。
    pub fn legacy_id(label: &[u8]) -> u32 {
        let label = terminated(label);
        [b"bgm".as_slice(), b"se", b"voice", b"sysvoice"]
            .iter()
            .position(|name| label.eq_ignore_ascii_case(name))
            .map_or(5, |index| index as u32)
    }
}
