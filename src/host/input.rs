use crate::{Error, Result};

/// Device order in a key binding table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum ButtonSource {
    Keyboard,
    Mouse,
    Joypad,
    Touch,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeviceBinding {
    pub enabled: u32,
    /// Indexed by device, then by binding group (0 to 3).
    pub codes: [[u8; 4]; 4],
    pub repeat: [[u32; 4]; 4],
}

/// Raw event mask and initial repeat flag returned by a device query.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeySample {
    pub mask: u32,
    pub repeat: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PointerSample {
    pub position: [i32; 2],
    pub active: u32,
}

/// Synchronous input queries. Each call can observe changes made by previous
/// calls or intervening script callbacks.
pub trait InputDevice {
    /// Borrow the current bindings for configuration saving. Return `None`
    /// to use the startup configuration. This must not sample input.
    fn key_configuration(&self) -> Option<&[DeviceBinding; 80]> {
        None
    }

    fn poll(&mut self) -> Result<()>;
    fn pointer(&mut self) -> Result<PointerSample>;
    /// `group` indexes one of four bindings. The event mask is queried even
    /// when `repeat` is false.
    fn key(&mut self, key: u32, group: u32, repeat: bool) -> Result<KeySample>;
    fn take_wheel(&mut self) -> Result<u32>;
    fn is_foreground(&mut self) -> Result<bool>;
    fn finish(&mut self) -> Result<()>;
    fn commit(&mut self) -> Result<()>;
    fn history(&mut self) -> Result<[u8; 32]>;
    fn show_cursor(&mut self, visible: u32) -> Result<()>;
    fn cursor_visible(&mut self) -> Result<u32>;
    fn set_pointer(&mut self, position: [i32; 2]) -> Result<()>;
    fn clear_keys(&mut self) -> Result<()>;
    /// Cancel delayed wheel repeat, keeping arrived events and key state.
    fn cancel_wheel_repeat(&mut self) -> Result<()> {
        Err(Error::new(0, "输入设备未提供滚轮重复取消"))
    }
    /// Reset touch contacts without clearing mouse or key state.
    fn clear_touches(&mut self) -> Result<()> {
        Err(Error::new(0, "输入设备未提供触摸重置"))
    }
}
