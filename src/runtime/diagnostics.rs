use super::{ObjectFactory, Runtime};
use std::{fmt::Arguments, rc::Rc};

/// Selects a family of optional diagnostic probes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceCategory {
    Input,
    NativeDetail,
    Message,
    Fes,
    FesBody,
    FesStep,
    Movie,
}

type TraceWriter = dyn Fn(TraceCategory, Arguments<'_>);

/// A host callback for human-readable diagnostics. The text is intended for
/// display and may change. Use `NativeTraceEvent` for structured call tracing.
#[derive(Clone)]
pub struct DiagnosticTrace {
    categories: Rc<[TraceCategory]>,
    write: Rc<TraceWriter>,
}

impl DiagnosticTrace {
    pub fn new(
        categories: &[TraceCategory],
        write: impl Fn(TraceCategory, Arguments<'_>) + 'static,
    ) -> Self {
        Self {
            categories: Rc::from(categories),
            write: Rc::new(write),
        }
    }

    pub(crate) fn enabled(&self, category: TraceCategory) -> bool {
        self.categories.contains(&category)
    }

    pub(crate) fn write(&self, category: TraceCategory, message: Arguments<'_>) {
        (self.write)(category, message);
    }
}

macro_rules! trace_detail {
    ($trace:expr, $category:ident, $($message:tt)+) => {{
        let category = $crate::runtime::TraceCategory::$category;
        if let Some(trace) = $trace.filter(|trace| trace.enabled(category)) {
            trace.write(category, format_args!($($message)+));
        }
    }};
}
pub(crate) use trace_detail;

/// Script diagnostics, in emission order within each encoding.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ScriptMessages {
    pub bytes: Vec<Vec<u8>>,
    pub utf16: Vec<Vec<u16>>,
}

/// A native call made by a runtime task.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeInvocation {
    pub task_id: u32,
    /// The native ID used by the script, before alias resolution.
    pub id: u32,
    /// Byte offset of the VM at dispatch.
    pub offset: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeTraceEvent {
    Enter(NativeInvocation),
    Exit {
        invocation: NativeInvocation,
        failed: bool,
    },
}

impl<F: ObjectFactory> Runtime<F> {
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
