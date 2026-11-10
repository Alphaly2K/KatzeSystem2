/// `SAVEPANEL` 的平面和启用编号；`ptype` 选择独立于单元内容。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SavePanelState {
    /// 面板自身的绘制平面。
    pub plane: crate::fes::CPlaneState,
    /// 面板底图与各槽位图像的资源引用。
    pub images: Vec<crate::fes::ImageReference>,
    /// 启用的单元编号，按有符号值排序且不重复。
    pub enabled_ids: std::collections::BTreeSet<i32>,
}

impl SavePanelState {
    pub fn display(&self) -> i32 {
        self.plane.display()
    }
}
