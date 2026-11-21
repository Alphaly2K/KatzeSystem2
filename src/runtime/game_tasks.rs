use super::{KcsFactory, ObjectFactory, ProgramLoader, RootObject, Runtime, TaskObject, fes, kind};
use crate::{
    Error, Result,
    audio::{Mixer, SoundFactory, SoundState},
    expression::State,
    graphics::GraphicsFactory,
    host::platform::WindowServices,
    input::InputFactory,
    kcs::Program,
    memory::SharedMemory,
};
use std::sync::Arc;

mod setup;

type FactoryChain<L> = SoundFactory<
    fes::FesFactory<GraphicsFactory<crate::message::MessageFactory<InputFactory<KcsFactory<L>>>>>,
>;

/// Standard object factory for a game session.
pub struct GameFactory<L>(FactoryChain<L>);

impl<L: ProgramLoader> ObjectFactory for GameFactory<L> {
    fn external_storage(&self) -> SharedMemory {
        self.0.external_storage()
    }

    fn create(&mut self, id: u32, kind: u32, name: &[u8]) -> Result<Box<dyn TaskObject>> {
        self.0.create(id, kind, name)
    }

    fn create_unloaded_ordinary(&mut self, id: u32) -> Result<Box<dyn TaskObject>> {
        self.0.create_unloaded_ordinary(id)
    }

    fn load_kcs_program(&mut self, name: &[u8]) -> Result<Option<Program>> {
        self.0.load_kcs_program(name)
    }
}

/// Task-level runtime with the standard game objects, for script tooling and diagnostics.
/// To embed a game, use [`crate::session::GameRuntime`].
pub type TaskRuntime<L = ResourcePrograms> = Runtime<GameFactory<L>>;

/// Loader marker for a session that obtains ordinary KCS through its resources.
pub struct ResourcePrograms(());

impl ProgramLoader for ResourcePrograms {
    fn load(&mut self, _: &[u8]) -> Result<Option<Program>> {
        Ok(None)
    }
}

/// Task IDs established at game startup. They remain valid until the tasks are destroyed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GameTasks {
    pub input: u32,
    pub sound: u32,
    pub entry: u32,
}

const ROOT_TASK: u32 = 0;
const INPUT_TASK: u32 = 1;
const SOUND_TASK: u32 = 2;

impl TaskRuntime {
    /// Prepare a resource-backed session with zeroed script storage.
    /// `shared_slots` counts 32-bit values; `external_bytes` counts bytes.
    /// Both storage areas are limited to 64 MiB each.
    pub fn prepare(shared_slots: usize, external_bytes: usize) -> Result<Self> {
        if external_bytes > crate::read::MAX_BYTES {
            return Err(Error::new(0, "共享存储超过上限"));
        }
        Self::with_resource_programs(shared_slots, SharedMemory::new(vec![0; external_bytes])?)
    }

    /// Prepare a game that reads and parses KCS through its resource service.
    /// Use this constructor to supply existing shared external storage.
    pub fn with_resource_programs(shared_slots: usize, external: SharedMemory) -> Result<Self> {
        let mut game = Self::with_programs(ResourcePrograms(()), shared_slots, external)?;
        game.resources.resource_programs = true;
        Ok(game)
    }

    pub(crate) fn has_resource_services(&self) -> bool {
        self.resources.services.is_some()
    }

    pub(crate) fn has_input_device(&self) -> bool {
        self.resources.input_device.is_some()
    }
}

impl<L: ProgramLoader> TaskRuntime<L> {
    /// Create the root and standard factory without opening any devices or resources.
    /// Attach host services and configure the canvas before calling [`Self::start_game`].
    pub fn with_programs(programs: L, shared_slots: usize, external: SharedMemory) -> Result<Self> {
        if shared_slots > crate::read::MAX_BYTES / size_of::<i32>() {
            return Err(Error::new(0, "共享变量区超过存储上限"));
        }
        Ok(Self::new(
            ROOT_TASK,
            kind::ROOT,
            Box::new(RootObject::new(INPUT_TASK, SOUND_TASK)),
            State::new(shared_slots),
            GameFactory(SoundFactory {
                enabled: true,
                fallback: fes::FesFactory {
                    fallback: GraphicsFactory {
                        fallback: crate::message::MessageFactory {
                            fallback: InputFactory {
                                fallback: KcsFactory { programs, external },
                            },
                        },
                    },
                },
            }),
        ))
    }

    /// Register input, sound and the named entry program in engine order.
    /// This consumes the prepared runtime. On error it drops any partially created tasks.
    /// Successful startup leaves the entry program ready for its first update.
    pub fn start_game(self, entry: &[u8]) -> Result<Self> {
        self.start_game_with_tasks(entry).map(|(game, _)| game)
    }

    /// Start the game and return the task IDs for diagnostics and script tools.
    pub fn start_game_with_tasks(mut self, entry: &[u8]) -> Result<(Self, GameTasks)> {
        if self.tree.root_id() != ROOT_TASK
            || self.tree.next_id() != INPUT_TASK
            || self
                .tree
                .children(ROOT_TASK)
                .is_none_or(|mut children| children.next().is_some())
        {
            return Err(Error::new(0, "游戏启动需要尚未创建子任务的运行时"));
        }
        let input = self.create(ROOT_TASK, kind::INPUT, b"input", b"")?;
        let sound = self.create(ROOT_TASK, kind::SOUND, b"sound", b"")?;
        let entry = self.create(ROOT_TASK, kind::ORDINARY, entry, b"")?;
        Ok((
            self,
            GameTasks {
                input,
                sound,
                entry,
            },
        ))
    }

    /// Transfer PCM consumption to a host mixer. Call after starting the game.
    pub fn connect_audio_output(&mut self) -> Result<Mixer> {
        self.connect_task_audio_output(SOUND_TASK)
    }

    /// Take back PCM consumption from the mixer for the standard sound task.
    pub fn disconnect_audio_output(&mut self, mixer: &mut Mixer) -> Result<()> {
        self.disconnect_task_audio_output(SOUND_TASK, mixer)
    }

    /// Fill a buffer with interleaved stereo `f32` PCM using the game mixer.
    /// See [`Runtime::mix_task_audio_stereo`] for buffer and timing requirements.
    /// A session without its sound task produces silence.
    pub fn mix_audio_stereo(&mut self, sample_rate: u32, output: &mut [f32]) -> Result<()> {
        if self.object(SOUND_TASK).is_none() {
            crate::audio::validate_stereo_output(sample_rate, output.len())?;
            output.fill(0.0);
            return Ok(());
        }
        self.mix_task_audio_stereo(SOUND_TASK, sample_rate, output)
    }

    /// Process pending window requests. Returns true when the host should exit.
    pub fn process_window_requests(&mut self, window: &mut dyn WindowServices) -> Result<bool> {
        self.process_window_requests_with_input(INPUT_TASK, window)
    }

    pub(crate) fn prepare_menu_close(&mut self) -> Result<()> {
        self.reset_menu_close_idle(INPUT_TASK)
    }

    /// Get a pending text entry request and its initial text, if any. Keep the
    /// returned handle until submission or cancellation and compare requests by
    /// identity with [`Arc::ptr_eq`]. The original dialog limits edits to
    /// 16 UTF-16 units.
    pub fn text_entry_request(&self) -> Option<Arc<str>> {
        self.input_state(INPUT_TASK)?.text_entry_request()
    }

    /// Submit text for this request, or pass `None` to cancel. Returns false
    /// if the input task has exited or the request is no longer current.
    pub fn complete_text_entry(&mut self, request: &Arc<str>, text: Option<&str>) -> Result<bool> {
        match self.input_state_mut(INPUT_TASK) {
            Some(input) => input.complete_text_entry(request, text),
            None => Ok(false),
        }
    }
}

impl<F: ObjectFactory> Runtime<F> {
    /// Transfer the specified sound task's PCM consumption to a host mixer.
    /// The mixer can be moved to the host's audio thread.
    /// Disconnect the current mixer before opening another one.
    pub fn connect_task_audio_output(&mut self, task: u32) -> Result<Mixer> {
        self.sound_output_state(task)?.connect_output()
    }

    /// Take back PCM consumption from the current mixer for a sound task.
    pub fn disconnect_task_audio_output(&mut self, task: u32, mixer: &mut Mixer) -> Result<()> {
        self.sound_output_state(task)?.disconnect_output(mixer)
    }

    /// Overwrite the buffer with interleaved left/right `f32` PCM in `[-1, 1]` and
    /// advance playback by the supplied frames. The sample rate is in Hz and must
    /// be nonzero; the buffer must contain an even number of samples.
    /// This may wait for decoded data. Returns an error for a missing sound task
    /// or one already connected to a mixer.
    pub fn mix_task_audio_stereo(
        &mut self,
        task: u32,
        sample_rate: u32,
        output: &mut [f32],
    ) -> Result<()> {
        self.sound_output_state(task)?
            .mix_stereo(sample_rate, output)
    }

    fn sound_output_state(&mut self, task: u32) -> Result<&mut SoundState> {
        self.object_mut(task)
            .and_then(|object| object.audio_state_mut())
            .ok_or_else(|| Error::new(0, format!("任务 {task} 没有声音状态")))
    }
}
