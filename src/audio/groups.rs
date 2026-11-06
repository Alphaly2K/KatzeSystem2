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
