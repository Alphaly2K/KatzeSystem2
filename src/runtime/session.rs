use super::{KcsFactory, ObjectFactory, ProgramLoader, RootObject, Runtime, TaskObject, fes, kind};
use crate::{
    Error, Result,
    audio::{SoundFactory, SoundState},
    expression::State,
    graphics::GraphicsFactory,
    input::InputFactory,
    kcs::Program,
    memory::SharedMemory,
};

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

/// Runtime with the standard game objects and a host-provided program loader.
pub type GameRuntime<L> = Runtime<GameFactory<L>>;

/// Task IDs established at game startup. They remain valid until the tasks are destroyed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GameTasks {
    pub input: u32,
    pub sound: u32,
    pub entry: u32,
}

/// Clock values supplied by the host for a single update.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameTime {
    /// Absolute game clock in logical ticks (normally 60 per second).
    pub tick: u32,
    /// Logical increment forwarded to tasks for this update.
    pub increment: u32,
    /// Wrapping millisecond clock for system queries and movies.
    pub system_time_ms: u32,
}

const ROOT_TASK: u32 = 0;
const INPUT_TASK: u32 = 1;
const SOUND_TASK: u32 = 2;

impl<L: ProgramLoader> GameRuntime<L> {
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
    pub fn start_game(mut self, entry: &[u8]) -> Result<(Self, GameTasks)> {
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
}

impl<F: ObjectFactory> Runtime<F> {
    /// Advance one host frame. Apply pending language changes before advancing the task tree.
    /// On a language error the failed request and remaining requests are retained for retry.
    /// Returns false once the root task has exited.
    pub fn advance(&mut self, time: FrameTime) -> Result<bool> {
        let mut requests = std::mem::take(&mut self.host.language_change_requests).into_iter();
        while let Some(request) = requests.next() {
            if let Err(error) = self.refresh_language(request.target) {
                self.host
                    .language_change_requests
                    .extend(std::iter::once(request).chain(requests));
                return Err(error);
            }
        }
        self.host.system_time_ms = time.system_time_ms;
        self.update(time.tick, time.increment)
    }

    /// Access a task's sound state to consume PCM or connect a separate mixer.
    pub fn sound_mut(&mut self, task: u32) -> Option<&mut SoundState> {
        self.object_mut(task)?.audio_state_mut()
    }
}
