use super::{HostState, Interpreter, NativeReturn, strings};
use crate::{Error, Result};

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

impl HostState {
    /// 绑定服务，供 KCS 和 FES 共用。替换服务后允许新服务初始化一次。
    pub fn set_achievement_services(&mut self, services: impl AchievementServices + 'static) {
        self.achievement_services = Some(Box::new(services));
        self.achievement_initialization_attempted = false;
    }

    pub(crate) fn initialize_achievements(&mut self, task_id: u32) -> Result<()> {
        if !self.achievement_initialization_attempted {
            if let Some(services) = &mut self.achievement_services {
                services.initialize(task_id)?;
            }
            self.achievement_initialization_attempted = true;
        }
        Ok(())
    }

    pub(crate) fn reset_achievements(&mut self, task_id: u32) -> Result<()> {
        if let Some(services) = &mut self.achievement_services {
            services.reset(task_id)?;
        }
        Ok(())
    }

    pub(crate) fn achievement_enabled(&self) -> Option<u32> {
        self.achievement_services
            .as_ref()
            .and_then(|services| services.enabled())
            .or(self.achievement_initialization_attempted.then_some(0))
    }

    pub(crate) fn achievement_broadcast(&mut self, task_id: u32) -> Result<u32> {
        let enabled = self.achievement_enabled();
        if enabled.is_some_and(|value| value != 0) {
            if let Some(services) = &mut self.achievement_services {
                services.poll_broadcast(task_id)?;
            }
        }
        Ok(if enabled.is_none() || self.debug_mode != 0 {
            self.steam_broadcast_override
        } else {
            0
        })
    }

    pub(crate) fn hook_achievement_screenshots(
        &mut self,
        task_id: u32,
        hooked: bool,
    ) -> Result<()> {
        if self.achievement_enabled().is_some_and(|value| value != 0) {
            if let Some(services) = &mut self.achievement_services {
                services.hook_screenshots(task_id, hooked)?;
            }
        }
        Ok(())
    }

    pub(crate) fn unlock_achievement(&mut self, task_id: u32, name: &[u8]) -> Result<()> {
        if let Some(services) = &mut self.achievement_services {
            services.unlock(task_id, name)?;
        }
        Ok(())
    }
}

pub(super) fn call(
    host: &mut HostState,
    index: u32,
    arguments: &[u8],
    vm: &Interpreter,
) -> Result<NativeReturn> {
    if matches!(index, 1041 | 1044) {
        super::native::words(arguments, 0, vm.pc)?;
        if index == 1041 {
            host.initialize_achievements(host.task_id)?;
            return Ok(NativeReturn::Continue);
        }
        host.reset_achievements(host.task_id)?;
        return Err(Error::new(vm.pc, "KCS native 1044 的原入口未初始化返回值"));
    }
    let args = super::native::words(arguments, 1, vm.pc)?;
    let name = strings::format(vm, args[0], 0)?;
    if index == 1042 {
        host.unlock_achievement(host.task_id, &name)?;
        Ok(NativeReturn::Continue)
    } else if let Some(services) = &mut host.achievement_services {
        Ok(NativeReturn::Value(u32::from(
            services.is_unlocked(host.task_id, &name)?,
        )))
    } else {
        Ok(NativeReturn::Value(0))
    }
}
