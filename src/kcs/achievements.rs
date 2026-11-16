use super::{HostState, Interpreter, NativeReturn, strings};
use crate::Result;

/// 成就服务维护已登记名称、解锁状态及平台可用性。
/// 名称保持格式展开后的原始字节，区分大小写。
pub trait AchievementServices {
    /// 首次解锁已登记名称时先更新本地状态，再提交解锁及保存请求。
    /// 未知、重复或服务不可用时保持状态；平台拒绝不回退本地状态。
    fn unlock(&mut self, task_id: u32, name: &[u8]) -> Result<()>;

    /// 返回本地解锁状态；未知名称或服务不可用时返回 false。
    fn is_unlocked(&mut self, task_id: u32, name: &[u8]) -> Result<bool>;
}

pub(super) fn call(
    host: &mut HostState,
    index: u32,
    arguments: &[u8],
    vm: &Interpreter,
) -> Result<NativeReturn> {
    let args = super::native::words(arguments, 1, vm.pc)?;
    let name = strings::format(vm, args[0], 0)?;
    let Some(services) = host.achievement_services.as_mut() else {
        return Ok(if index == 1042 {
            NativeReturn::Continue
        } else {
            NativeReturn::Value(0)
        });
    };
    if index == 1042 {
        services.unlock(host.task_id, &name)?;
        Ok(NativeReturn::Continue)
    } else {
        Ok(NativeReturn::Value(u32::from(
            services.is_unlocked(host.task_id, &name)?,
        )))
    }
}
