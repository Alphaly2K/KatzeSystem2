//! Host requests for windows, system integration and platform jobs.

use crate::{Error, Result};

/// Requests for platform work that runs independently of the script VM.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlatformJobRequest<'a> {
    /// The formatted request bytes, without a terminating zero.
    Plugin(&'a [u8]),
    /// Select and register an image in the host's import directory.
    ImageImport,
}

pub trait PlatformJob {
    /// Return None while pending, or the completed result (used by the plugin query).
    fn poll(&mut self) -> Option<u32>;
}

pub trait PlatformJobServices {
    /// None represents an unavailable plugin or an import that did not start.
    /// The returned job owns its work and must handle cancellation when dropped.
    fn start(
        &mut self,
        task_id: u32,
        request: PlatformJobRequest<'_>,
    ) -> Result<Option<Box<dyn PlatformJob>>>;
}

/// Window preferences from the game configuration. The host selects the
/// actual window and display modes independently of the logical canvas size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct WindowPreferences {
    pub window_size: [u32; 2],
    pub fullscreen_size: [u32; 2],
    pub fullscreen: bool,
}

/// Window operations supplied by the host after a runtime update.
pub trait WindowServices {
    /// Toggle fullscreen and report the resulting mode (true for fullscreen).
    fn toggle_fullscreen(&mut self) -> Result<bool>;
    fn minimize(&mut self) -> Result<()>;
    /// Return true to accept the requested close.
    fn confirm_close(&mut self) -> bool;
}

/// 交给窗口宿主的异步菜单项状态请求。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuItemRequest {
    pub task_id: u32,
    pub command_id: u32,
    /// 非零启用，零禁用；保留原始 DWORD。
    pub value: u32,
}

/// 普通 9 提交的窗口关闭请求；由平台宿主消费。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowCloseRequest {
    pub task_id: u32,
    /// 零参数使用应用退出通知，非零参数使用标准关闭消息。
    pub notify: bool,
}

/// 普通窗口入口提交的异步动作，由窗口宿主依序消费。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowAction {
    OpenSystemProperties,
    /// 重建时读取 `HostState::debug_mode` 的最新值。
    RebuildMenu,
    ToggleFullscreen,
    Minimize,
    /// 先向输入任务发送空闲重置命令，再执行标准窗口关闭流程。
    CloseFromMenu,
    /// 由窗口接收后向任务树广播命令 21。
    NotifyTasks,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowRequest {
    pub task_id: u32,
    pub action: WindowAction,
}

/// 复制请求的文本；不包含结尾零，UTF-16 保留独立代理单元。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClipboardText {
    /// 普通 10 的原始字节，由宿主按源编码解码。
    Bytes(Vec<u8>),
    Utf16(Vec<u16>),
}

/// 普通 10／1039 提交的剪贴板写入请求；宿主负责实际写入和失败处理。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipboardRequest {
    pub task_id: u32,
    pub text: ClipboardText,
}

/// 宿主本地的民用日期与时间；星期从周日（0）开始。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalDateTime {
    pub year: u16,
    pub month: u16,
    pub weekday: u16,
    pub day: u16,
    pub hour: u16,
    pub minute: u16,
    pub second: u16,
}

/// 普通脚本使用的同步平台服务。
pub trait SystemServices {
    /// 读取当地日期和时间。未提供该能力时返回 None。
    fn local_date_time(&mut self) -> Option<LocalDateTime> {
        None
    }

    /// 按系统 open 操作提交请求；平台返回的启动结果不参与 VM 返回值。
    fn open(
        &mut self,
        _task_id: u32,
        _target: &[u8],
        _parameters: &[u8],
        _directory: &[u8],
    ) -> Result<()> {
        Err(Error::new(0, "系统打开服务尚未接入"))
    }

    /// 显示标题为 system error 的单按钮对话框，关闭后返回；文字不含末尾零。
    /// 平台显示失败也正常结束；Err 用于宿主集成诊断。
    fn message_box(&mut self, _task_id: u32, _text: &[u16]) -> Result<()> {
        Err(Error::new(0, "系统对话框服务尚未接入"))
    }
}

/// 成就服务维护已登记名称、解锁状态及平台可用性。
/// 名称保持格式展开后的原始字节，区分大小写。
pub trait AchievementServices {
    /// 尝试初始化平台并建立管理器。平台不可用也完成本次尝试，返回 `Ok(())`。
    /// 已由宿主完成初始化的服务可沿用默认实现。
    fn initialize(&mut self, _task_id: u32) -> Result<()> {
        Ok(())
    }

    /// 管理器的原始启用字段。`None` 表示服务未建立管理器。
    fn enabled(&self) -> Option<u32> {
        None
    }

    /// 查询时轮询已启用的平台接口；脚本返回值来自引擎的配信字段。
    fn poll_broadcast(&mut self, _task_id: u32) -> Result<()> {
        Ok(())
    }

    /// 设置平台截图是否由游戏接管。平台不可用时保持状态。
    fn hook_screenshots(&mut self, _task_id: u32, _hooked: bool) -> Result<()> {
        Ok(())
    }

    /// 首次解锁已登记名称时先更新本地状态，再提交解锁及保存请求。
    /// 未知、重复或服务不可用时保持状态；平台拒绝不回退本地状态。
    fn unlock(&mut self, task_id: u32, name: &[u8]) -> Result<()>;

    /// 返回本地解锁状态；未知名称或服务不可用时返回 false。
    fn is_unlocked(&mut self, task_id: u32, name: &[u8]) -> Result<bool>;

    /// 管理器已启用且有状态表时清除本地标记，保留已登记的名称。
    /// 随后请求平台重置；平台接口缺失或拒绝时保留本地清零。
    fn reset(&mut self, _task_id: u32) -> Result<()> {
        Err(Error::new(0, "宿主成就服务未实现清零"))
    }
}
