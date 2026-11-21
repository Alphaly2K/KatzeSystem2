//! Execution policy for media decoding in a session.

/// Choose how the runtime decodes Vorbis streams and movies.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DecodeMode {
    /// Decode on the session thread. Vorbis is prepared as bounded PCM when
    /// loaded. Movies decode incrementally as the session advances.
    Inline,
    /// Prefetch streamed Vorbis and movies on native worker threads.
    /// Uses inline decoding on bare WebAssembly targets.
    #[default]
    Threaded,
}

impl DecodeMode {
    pub(crate) fn uses_inline(self) -> bool {
        self == Self::Inline || cfg!(all(target_family = "wasm", target_os = "unknown"))
    }
}
