use super::SliderState;

/// SCROLL2 复用滑动控件的属性与更新请求，范围另外保存页长。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scroll2State {
    pub control: SliderState,
    pub page: i32,
}

impl Scroll2State {
    pub fn set_page(&mut self, range: [i32; 2], page: i32) {
        self.control.set_range(range);
        self.page = page;
    }
}
