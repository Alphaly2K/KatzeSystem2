use super::{KcsFactory, ObjectFactory, ProgramLoader, RootObject, Runtime, TaskObject, fes, kind};
use crate::{
    Error, Result,
    audio::{Mixer, SoundConfig, SoundFactory, SoundGroups, SoundState},
    expression::State,
    graphics::GraphicsFactory,
    input::InputFactory,
    kcs::{CachedResources, Program, ResourceServices},
    language::{LanguageState, StringTableConfig},
    memory::SharedMemory,
    resource::XmlDocument,
    settings::SystemSettings,
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

    /// Apply the engine settings before starting the game. Optional game defaults
    /// and user settings override the startup document in that order. The caller
    /// supplies document bytes and separately binds resources and devices.
    /// Invalid input leaves the current settings unchanged.
    pub fn configure_startup(
        &mut self,
        startup: &[u8],
        game_defaults: Option<&[u8]>,
        user_settings: Option<&[u8]>,
    ) -> Result<()> {
        if self.tree.next_id() != INPUT_TASK {
            return Err(Error::new(0, "启动配置必须在创建游戏任务前应用"));
        }
        let mut settings = SystemSettings::default();
        settings.apply_startup_xml(startup)?;
        if let Some(bytes) = game_defaults {
            settings.apply_game_xml(bytes)?;
        }
        let mut languages = LanguageState::from_xml(startup)?;
        if let Some(bytes) = user_settings {
            settings.apply_xml(bytes)?;
            languages.apply_user_xml(bytes)?;
        }
        settings.refresh_sound_volumes();

        let xml = XmlDocument::parse(startup)?;
        let unicode_text = xml.integer(b"APP/utfmode", 0)?;
        let message_time_scale = xml.integer(b"GAME/strframe_frame", 0)?;
        let string_table_config = StringTableConfig::from_xml(startup)?;
        let mut message_defaults = Default::default();
        settings.apply_message_defaults(&mut message_defaults);

        self.host.languages = languages;
        self.host.string_table_config = Some(string_table_config);
        self.host.unicode_text = unicode_text;
        self.host.message_time_scale = Some(message_time_scale);
        self.host.message_defaults = Some(message_defaults);
        self.host.system_settings = Some(settings);
        Ok(())
    }

    /// The effective engine settings, if startup settings have been applied.
    pub fn system_settings(&self) -> Option<&SystemSettings> {
        self.host.system_settings.as_ref()
    }

    /// Bind the game's resource service and sound configuration before startup.
    /// The service resolves archive categories and opens resources. The runtime
    /// selects the category for each sound name using the supplied definition.
    /// With no definition, it uses startup sound groups and uncategorized names.
    /// Invalid XML leaves both the active configuration and resource service unchanged.
    pub fn set_game_resources(
        &mut self,
        services: impl ResourceServices + 'static,
        startup: &[u8],
        sound_definition: Option<&[u8]>,
        sound_user_settings: Option<&[u8]>,
    ) -> Result<()> {
        if self.tree.next_id() != INPUT_TASK {
            return Err(Error::new(0, "游戏资源必须在创建游戏任务前绑定"));
        }
        let (definition, sound, groups) = if let Some(bytes) = sound_definition {
            let definition = SoundConfig::from_xml(bytes)?;
            let mut sound = definition.clone();
            if let Some(user) = sound_user_settings {
                sound.apply_user_settings_xml(user)?;
            }
            (Some(definition), Some(sound), SoundGroups::from_xml(bytes)?)
        } else {
            (None, None, SoundGroups::from_startup_xml(startup)?)
        };
        self.resources.services = Some(Box::new(
            CachedResources::new(services).with_sound_names(definition),
        ));
        self.resources.animation_aliases = None;
        self.host.sound_config_enabled = u32::from(sound.is_some());
        self.host.sound_config = sound;
        self.host.sound_groups = Some(groups);
        Ok(())
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

    /// Transfer PCM consumption to a mixer owned by the host. For a standard game,
    /// pass [`GameTasks::sound`]. The mixer can be moved to the host's audio thread.
    /// A sound task can only be connected once, even if the mixer has been dropped.
    pub fn connect_audio_output(&mut self, task: u32) -> Result<Mixer> {
        self.sound_output_state(task)?.connect_output()
    }

    /// Overwrite the buffer with interleaved left/right `f32` PCM in `[-1, 1]` and
    /// advance playback by the supplied frames. The sample rate is in Hz and must
    /// be nonzero; the buffer must contain an even number of samples.
    /// This may wait for decoded data. Returns an error for a missing sound task
    /// or one already connected to a mixer.
    pub fn mix_audio_stereo(
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
