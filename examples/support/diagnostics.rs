use katze_system2::runtime::{
    DiagnosticTrace, NativeTraceEvent, ObjectFactory, Runtime, TraceCategory,
};
use std::{cell::RefCell, time::Instant};

pub struct Output {
    pub timing: bool,
    scripts: bool,
    probes: Vec<TraceCategory>,
    encoding: &'static encoding_rs::Encoding,
}

impl Output {
    pub fn from_env(encoding: &'static encoding_rs::Encoding) -> Self {
        Self {
            timing: std::env::var_os("KATZE_TRACE_TIMING").is_some(),
            scripts: std::env::var_os("KATZE_TRACE_SCRIPT").is_some(),
            probes: [
                ("KATZE_TRACE_INPUT", TraceCategory::Input),
                ("KATZE_TRACE_NATIVE", TraceCategory::NativeDetail),
                ("KATZE_TRACE_MSG", TraceCategory::Message),
                ("KATZE_TRACE_FES", TraceCategory::Fes),
                ("KATZE_TRACE_FES_BODY", TraceCategory::FesBody),
                ("KATZE_TRACE_FES_STEP", TraceCategory::FesStep),
            ]
            .into_iter()
            .filter_map(|(name, category)| std::env::var_os(name).map(|_| category))
            .collect(),
            encoding,
        }
    }

    pub fn connect<F: ObjectFactory>(&self, runtime: &mut Runtime<F>) {
        if !self.probes.is_empty() {
            runtime.set_diagnostic_trace(Some(DiagnosticTrace::new(&self.probes, |_, message| {
                eprintln!("{message}");
            })));
        }
        if self.timing {
            let calls = RefCell::new(Vec::new());
            runtime.set_native_trace(move |event| match event {
                NativeTraceEvent::Enter(_) => calls.borrow_mut().push(Instant::now()),
                NativeTraceEvent::Exit { invocation, .. } => {
                    let elapsed = calls.borrow_mut().pop().unwrap().elapsed();
                    if elapsed.as_millis() >= 20 {
                        eprintln!(
                            "[slow-native] task={} native={} ms={:.2}",
                            invocation.task_id,
                            invocation.id,
                            elapsed.as_secs_f64() * 1000.0
                        );
                    }
                }
            });
        }
    }

    pub fn drain<F: ObjectFactory>(&self, runtime: &mut Runtime<F>) {
        let messages = runtime.take_script_messages();
        if self.scripts {
            for bytes in messages.bytes {
                eprintln!("[script] {}", self.encoding.decode(&bytes).0);
            }
            for units in messages.utf16 {
                eprintln!(
                    "[script] {}",
                    String::from_utf16_lossy(&units).trim_end_matches('\n')
                );
            }
        }
    }
}
