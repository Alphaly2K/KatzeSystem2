//! Script diagnostics and the optional quiz debug panel.

pub use crate::runtime::{QuizDebugInput, QuizDebugSession};
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
        let category = $crate::host::TraceCategory::$category;
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

/// Current execution location for a loaded script task.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScriptPosition {
    pub task_id: u32,
    pub location: ScriptLocation,
}

/// KCS positions are byte offsets. FES positions are indices in the program
/// and its current block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptLocation {
    Kcs {
        byte_offset: usize,
    },
    Fes {
        block: usize,
        instruction: usize,
        pending: bool,
    },
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
