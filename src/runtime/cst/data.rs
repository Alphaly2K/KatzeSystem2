use super::CstContext;
use crate::kcs::{Interpreter, NativeReturn};
use crate::runtime::{ObjectFactory, ObjectHost};
use crate::{Error, Result, read, save::DataContainer};

impl CstContext {
    fn clear_entry_data(&mut self) {
        for entry in self.registrations.iter_mut().chain(&mut self.history) {
            entry.saved_data = Vec::new();
        }
    }
}

impl<F: ObjectFactory> ObjectHost<'_, F> {
    pub(super) fn cst_data_native(
        &mut self,
        index: u32,
        arguments: &[u8],
        vm: &Interpreter,
    ) -> Result<NativeReturn> {
        let handle = read::u32_at(arguments, 0)?;
        if arguments.len() != 8 && !(handle == 0 && arguments.len() == 4) {
            return Err(Error::new(vm.pc, "KCS native 参数字节数不匹配"));
        }
        let empty = if index == 979 {
            NativeReturn::Value(0)
        } else {
            NativeReturn::Continue
        };
        if handle == 0 {
            return Ok(empty);
        }
        let source = read::u32_at(arguments, 4)?;
        if index == 981 {
            if source == 0 {
                return Ok(empty);
            }
            let source_context = self
                .resources
                .cst_contexts
                .get(&source)
                .ok_or_else(|| Error::new(vm.pc, "CST 上下文句柄无效"))?;
            if source == handle {
                // 原循环先释放目标数据并清零长度，源目标相同时因此得到空数据。
                self.resources
                    .cst_contexts
                    .get_mut(&handle)
                    .unwrap()
                    .clear_entry_data();
            } else {
                let registrations: Vec<_> = source_context
                    .registrations
                    .iter()
                    .map(|entry| entry.saved_data.clone())
                    .collect();
                let history: Vec<_> = source_context
                    .history
                    .iter()
                    .map(|entry| entry.saved_data.clone())
                    .collect();
                let target = self
                    .resources
                    .cst_contexts
                    .get_mut(&handle)
                    .ok_or_else(|| Error::new(vm.pc, "CST 上下文句柄无效"))?;
                for (entry, bytes) in target
                    .registrations
                    .iter_mut()
                    .zip(registrations)
                    .chain(target.history.iter_mut().zip(history))
                {
                    entry.saved_data = bytes;
                }
            }
            return Ok(NativeReturn::Continue);
        }
        let context = self
            .resources
            .cst_contexts
            .get_mut(&handle)
            .ok_or_else(|| Error::new(vm.pc, "CST 上下文句柄无效"))?;
        if index == 978 {
            if source != 0 {
                let bytes = self.host.data_container(source)?.snapshot()?;
                if let Some(entry) = context.history.last_mut() {
                    entry.saved_data = bytes;
                }
            }
            return Ok(NativeReturn::Continue);
        }
        let Some(entry) = context
            .history
            .get(source as usize)
            .filter(|entry| !entry.saved_data.is_empty())
        else {
            return Ok(NativeReturn::Value(0));
        };
        let container = DataContainer::from_snapshot(&entry.saved_data)?;
        Ok(NativeReturn::Value(
            self.host.insert_data_container(container, vm.pc)?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kcs::Program;
    use crate::runtime::{BaseObject, KcsFactory, ProgramLoader, Runtime, kind};

    struct Loader;
    impl ProgramLoader for Loader {
        fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
            let mut code = Vec::new();
            for address in [0u32, 4] {
                code.extend([0x25, 0x13]);
                code.extend(address.to_le_bytes());
            }
            code.extend([0x2b, 3]);
            code.extend(981u32.to_le_bytes());
            code.push(3);
            code.extend(8u32.to_le_bytes());
            code.push(0x3b);
            Ok(Some(Program::from_code(&code, vec![0; 8], 16)?))
        }
    }

    fn context(data: [&[&[u8]]; 2]) -> CstContext {
        let mut context = CstContext::new(&[]);
        for (entries, data) in [&mut context.registrations, &mut context.history]
            .into_iter()
            .zip(data)
        {
            for (i, bytes) in data.iter().enumerate() {
                entries.push(super::super::Entry {
                    name: vec![b'A' + i as u8],
                    value: i as u32,
                    details: vec![0x41; 72],
                    saved_data: bytes.to_vec(),
                });
            }
        }
        context
    }

    #[test]
    fn data_copy_handles_both_lists_without_replacing_names_details_or_unpaired_entries() {
        for self_copy in [false, true] {
            let mut runtime = Runtime::new(
                0,
                kind::ROOT,
                Box::new(BaseObject::new()),
                crate::expression::State::new(0),
                KcsFactory::new(Loader),
            );
            let target = runtime.host_mut().handle_allocator().allocate().unwrap();
            let source = runtime.host_mut().handle_allocator().allocate().unwrap();
            runtime.resources.cst_contexts.insert(
                target,
                context([&[b"one", b"", b"tail"], &[b"old1", b"old2"]]),
            );
            runtime.resources.cst_contexts.insert(
                source,
                context([&[b"", b"two"], &[b"new1", b"new2", b"unused"]]),
            );
            let task = runtime.create(0, kind::ORDINARY, b"main", b"").unwrap();
            let vm = runtime.object_mut(task).unwrap().interpreter_mut().unwrap();
            vm.write(0, &target.to_le_bytes()).unwrap();
            vm.write(4, &if self_copy { target } else { source }.to_le_bytes())
                .unwrap();
            runtime.update(0, 0).unwrap();
            let context = &runtime.resources.cst_contexts[&target];
            for entries in [&context.registrations, &context.history] {
                for (i, entry) in entries.iter().enumerate() {
                    assert_eq!(entry.name, [b'A' + i as u8]);
                    assert_eq!(entry.value, i as u32);
                    assert_eq!(entry.details, [0x41; 72]);
                }
            }
            if self_copy {
                assert!(
                    context
                        .registrations
                        .iter()
                        .chain(&context.history)
                        .all(|entry| entry.saved_data.is_empty())
                );
            } else {
                assert_eq!(
                    context
                        .registrations
                        .iter()
                        .map(|entry| entry.saved_data.as_slice())
                        .collect::<Vec<_>>(),
                    [b"".as_slice(), b"two", b"tail"]
                );
                assert_eq!(
                    context
                        .history
                        .iter()
                        .map(|entry| entry.saved_data.as_slice())
                        .collect::<Vec<_>>(),
                    [b"new1".as_slice(), b"new2"]
                );
                runtime
                    .resources
                    .cst_contexts
                    .get_mut(&source)
                    .unwrap()
                    .history[0]
                    .saved_data[0] = b'X';
                assert_eq!(
                    runtime.resources.cst_contexts[&target].history[0].saved_data,
                    b"new1"
                );
            }
        }
    }
}
