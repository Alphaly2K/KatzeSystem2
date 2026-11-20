use super::{ObjectFactory, Runtime};
use crate::host::{
    DiagnosticTrace, NativeTraceEvent, ScriptLocation, ScriptMessages, ScriptPosition,
};
use std::rc::Rc;

impl<F: ObjectFactory> Runtime<F> {
    /// Inspect registered script tasks without advancing or modifying them.
    /// After a failure a task's current position may differ from the error offset.
    pub fn script_positions(&self) -> impl Iterator<Item = ScriptPosition> + '_ {
        let root = self.tree.root_id();
        self.tree
            .descendants(root)
            .into_iter()
            .flat_map(move |tasks| std::iter::once(root).chain(tasks))
            .filter_map(|task_id| {
                let object = self.object(task_id)?;
                let location = if let Some(vm) = object.interpreter() {
                    ScriptLocation::Kcs { byte_offset: vm.pc }
                } else {
                    let (pending, block, instruction) =
                        object.fes_scene()?.interpreter().pending_pc();
                    ScriptLocation::Fes {
                        block,
                        instruction,
                        pending,
                    }
                };
                Some(ScriptPosition { task_id, location })
            })
    }

    /// Set optional diagnostic output for this runtime. Pass `None` to disable it.
    pub fn set_diagnostic_trace(&mut self, trace: Option<DiagnosticTrace>) {
        self.resources.diagnostic_trace = trace;
    }

    /// Drain script messages without converting encodings or displaying output.
    pub fn take_script_messages(&mut self) -> ScriptMessages {
        ScriptMessages {
            bytes: std::mem::take(&mut self.host.debug_messages),
            utf16: std::mem::take(&mut self.host.unicode_debug_messages),
        }
    }

    /// Observe native calls on the runtime thread. Calls may nest; each normal
    /// return, including an error, emits a matching exit event. The host owns
    /// timing and output policy. No observer is installed by default.
    pub fn set_native_trace(&mut self, trace: impl Fn(NativeTraceEvent) + 'static) {
        self.resources.native_trace = Some(Rc::new(trace));
    }

    pub fn clear_native_trace(&mut self) {
        self.resources.native_trace = None;
    }
}
