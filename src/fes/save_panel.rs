mod content;
pub use content::PanelContent;

/// `SAVEPANEL` 的平面、单元定义与槽位数据。
#[derive(Clone, Debug, PartialEq)]
pub struct SavePanelState {
    /// 面板自身的绘制平面。
    pub plane: crate::fes::CPlaneState,
    /// 面板底图与各槽位图像的资源引用。
    pub images: Vec<crate::fes::ImageReference>,
    /// 启用的单元编号，按有符号值排序且不重复。
    pub enabled_ids: std::collections::BTreeSet<i32>,
    /// 单元内容按编号保存，与启用选择分别更新。
    pub cells: std::collections::BTreeMap<i32, PanelCell>,
    /// 来源应用后的内容；重定义单元不立即替换该记录。
    pub contents: std::collections::BTreeMap<i32, PanelContent>,
    /// 最近请求的存档文件名，使用 CP932 字节。
    pub save_name: Vec<u8>,
    /// 最近请求读取的数值记录编号。
    pub record_id: u32,
    /// 面板独立的 1024 项数值区。
    pub flags: Vec<i32>,
    /// 文件修改时间的本地年、月、日、时、分、秒。
    pub date: [u32; 6],
    /// 最近成功装载的 CSS 数据；文件缺失时保留。
    pub data: Option<crate::save::DataContainer>,
}

impl Default for SavePanelState {
    fn default() -> Self {
        Self {
            plane: Default::default(),
            images: Vec::new(),
            enabled_ids: Default::default(),
            cells: Default::default(),
            contents: Default::default(),
            save_name: Vec::new(),
            record_id: 0,
            flags: vec![0; 1024],
            date: [0; 6],
            data: None,
        }
    }
}

/// 不含进程地址的原生面板单元记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelCell {
    pub(crate) data: [u8; 188],
}

impl Default for PanelCell {
    fn default() -> Self {
        Self { data: [0; 188] }
    }
}

impl PanelCell {
    pub fn record(&self) -> &[u8; 188] {
        &self.data
    }

    pub fn kind(&self) -> u32 {
        u32::from_le_bytes(self.data[..4].try_into().unwrap())
    }

    pub fn source_kind(&self) -> u32 {
        u32::from_le_bytes(self.data[4..8].try_into().unwrap())
    }

    fn word(&self, offset: usize) -> u32 {
        u32::from_le_bytes(self.data[offset..offset + 4].try_into().unwrap())
    }
}

impl SavePanelState {
    pub fn display(&self) -> i32 {
        self.plane.display()
    }
}
