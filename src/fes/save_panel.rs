/// `SAVEPANEL` 存档面板：由 FES 脚本逐格配置图像、数值与文字单元；
/// 单元类型与资源绑定按 `ptype` 命令记录，显示与槽位数据后续接入。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SavePanelState {
    /// 面板自身的绘制平面。
    pub plane: crate::fes::CPlaneState,
    /// 面板底图与各槽位图像的资源引用。
    pub images: Vec<crate::fes::ImageReference>,
    /// `ptype` 命令登记的单元类型参数。
    pub cells: Vec<Vec<i32>>,
}

impl SavePanelState {
    pub fn display(&self) -> i32 {
        self.plane.display()
    }
}
