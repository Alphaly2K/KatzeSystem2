/// `SAVEPANEL` 的平面和启用编号；`dispid` 选择独立于单元内容。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SavePanelState {
    /// 面板自身的绘制平面。
    pub plane: crate::fes::CPlaneState,
    /// 面板底图与各槽位图像的资源引用。
    pub images: Vec<crate::fes::ImageReference>,
    /// 启用的单元编号，按有符号值排序且不重复。
    pub enabled_ids: std::collections::BTreeSet<i32>,
    /// 单元内容按编号保存，与启用选择分别更新。
    pub cells: std::collections::BTreeMap<i32, PanelCell>,
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
}

impl SavePanelState {
    pub fn display(&self) -> i32 {
        self.plane.display()
    }
}
