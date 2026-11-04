use super::{ObjectFactory, ObjectHost};
use crate::kcs::{Interpreter, NativeReturn};
use crate::{Error, Result, read};
use std::collections::BTreeSet;
use std::sync::Arc;

/// 独立 CST 上下文，语言注册在构造时从全局前缀复制。
pub struct CstContext {
    languages: BTreeSet<Vec<u8>>,
    script: Option<Arc<crate::cst::Script>>,
}

impl CstContext {
    fn new(prefixes: &[Vec<u8>]) -> Self {
        Self {
            languages: prefixes
                .iter()
                .map(|prefix| {
                    prefix[..prefix.iter().position(|&b| b == 0).unwrap_or(prefix.len())].to_vec()
                })
                .collect(),
            script: None,
        }
    }

    /// 按原始字节顺序枚举去重后的语言名，包括已注册的空名称。
    pub fn languages(&self) -> impl ExactSizeIterator<Item = &[u8]> {
        self.languages.iter().map(Vec::as_slice)
    }

    pub fn script(&self) -> Option<&crate::cst::Script> {
        self.script.as_deref()
    }
}

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn cst_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &mut Interpreter,
    ) -> Result<NativeReturn> {
        if index == 323 {
            if self.resources.cst_contexts.len() >= read::MAX_ITEMS {
                return Err(Error::new(vm.pc, "CST 上下文数量超过上限"));
            }
            let context = CstContext::new(&self.host.languages.prefixes);
            let handle = self.host.handle_allocator().allocate()?;
            self.resources.cst_contexts.insert(handle, context);
            return Ok(NativeReturn::Value(handle));
        }
        let handle = read::u32_at(arguments, 0)?;
        if handle != 0 && self.resources.cst_contexts.remove(&handle).is_none() {
            return Err(Error::new(vm.pc, "CST 上下文句柄无效"));
        }
        Ok(NativeReturn::Continue)
    }
}
