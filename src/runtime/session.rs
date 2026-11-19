use super::{
    KcsFactory, ObjectFactory, ProgramLoader, RootObject, Runtime, TaskObject, WindowServices, fes,
    kind,
};
use crate::{
    Error, Result,
    audio::{Mixer, SoundConfig, SoundFactory, SoundGroups, SoundState},
    expression::State,
    graphics::{DrawingContext, GraphicsFactory, Projection, RenderTarget, TextureFilter},
    input::{DeviceBinding, DeviceKeys, InputFactory},
    kcs::{CachedResources, Program, ResourceServices},
    language::{LanguageState, StringTableConfig},
    memory::SharedMemory,
    resource::XmlDocument,
    settings::SystemSettings,
};
use std::sync::Arc;

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

/// Runtime with the standard game objects. The default reads compiled scripts
/// through the same resource service as the other game assets.
pub type GameRuntime<L = ResourcePrograms> = Runtime<GameFactory<L>>;

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

/// The outcome of advancing a session by one frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameOutcome {
    Running {
        /// Interface strings were refreshed since the previous successful advance.
        /// The host can now read the current tables to update its UI.
        interface_language_refreshed: bool,
    },
    /// The root task has exited. The host should stop advancing and presenting.
    Exited,
}

const ROOT_TASK: u32 = 0;
const INPUT_TASK: u32 = 1;
const SOUND_TASK: u32 = 2;

impl GameRuntime {
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
}

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

    /// Apply engine settings and input bindings before starting the game. Optional
    /// game defaults override engine settings; user settings override both engine
    /// settings and input bindings. The caller supplies the document bytes and
    /// separately binds resources and devices.
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
        let mut keys = DeviceKeys::from_xml(startup)?;
        if let Some(bytes) = user_settings {
            settings.apply_xml(bytes)?;
            languages.apply_user_xml(bytes)?;
            keys.apply_user_xml(bytes)?;
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
        self.host.configuration_keys = Some(keys);
        Ok(())
    }

    /// Startup bindings with user overrides applied. Pass these to the input
    /// device when constructing it. Saving prefers the attached device's current
    /// bindings when available and otherwise uses these bindings.
    pub fn startup_input_bindings(&self) -> Option<&[DeviceBinding; 80]> {
        self.host
            .configuration_keys
            .as_ref()
            .map(DeviceKeys::bindings)
    }

    /// The effective engine settings, if startup settings have been applied.
    pub fn system_settings(&self) -> Option<&SystemSettings> {
        self.host.system_settings.as_ref()
    }

    /// Prepare the standard canvas and frame view for a logical surface size in pixels.
    /// Call after applying startup settings. A configured tile grid determines the
    /// canvas size; without a grid the canvas uses the logical surface size.
    /// This replaces the existing canvas on success and preserves it on error.
    pub fn configure_canvas(&mut self, surface_size: [u32; 2]) -> Result<()> {
        let target = match self.system_settings() {
            Some(settings) if !settings.texture_grid.contains(&0) => {
                let [columns, rows] = settings.texture_grid;
                let count = (columns as usize)
                    .checked_mul(rows as usize)
                    .filter(|&count| count <= crate::read::MAX_ITEMS)
                    .ok_or_else(|| Error::new(0, "画布面块数量超过上限"))?;
                let mut target = RenderTarget::from_tiles(
                    settings.texture_size,
                    settings.texture_grid,
                    vec![None; count],
                )?;
                target.prepare_tiles(&settings.texture_areas, settings.texture_16bit_row)?;
                target
            }
            _ => RenderTarget::new(surface_size)?,
        };
        self.set_surface_size(surface_size);
        *self.drawing_mut() = DrawingContext {
            mode: 0,
            enabled: true,
            active: true,
            target: Some(target),
        };
        let projection = Projection::default();
        let parent = projection.matrix;
        self.set_frame_view(projection, parent, TextureFilter::Point);
        Ok(())
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

    /// Fill a buffer with interleaved stereo `f32` PCM using the game mixer.
    /// See [`Runtime::mix_task_audio_stereo`] for buffer and timing requirements.
    pub fn mix_audio_stereo(&mut self, sample_rate: u32, output: &mut [f32]) -> Result<()> {
        self.mix_task_audio_stereo(SOUND_TASK, sample_rate, output)
    }

    /// Process pending window requests. Returns true when the host should exit.
    pub fn process_window_requests(&mut self, window: &mut dyn WindowServices) -> Result<bool> {
        self.process_window_requests_with_input(INPUT_TASK, window)
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
    /// Advance one host frame. Apply pending language changes before advancing the task tree.
    /// On a language error the failed request and remaining requests are retained for retry.
    /// Completed interface refreshes are still reported after a failed advance.
    /// After the root has exited, subsequent calls return [`FrameOutcome::Exited`].
    pub fn advance(&mut self, time: FrameTime) -> Result<FrameOutcome> {
        if self.tree.node(self.tree.root_id()).is_none() {
            return Ok(FrameOutcome::Exited);
        }
        self.apply_pending_language_changes()?;
        self.host.system_time_ms = time.system_time_ms;
        let active = self.update(time.tick, time.increment)?;
        let interface_language_refreshed = std::mem::take(&mut self.interface_language_refreshed);
        Ok(if active {
            FrameOutcome::Running {
                interface_language_refreshed,
            }
        } else {
            FrameOutcome::Exited
        })
    }

    /// Transfer the specified sound task's PCM consumption to a host mixer.
    /// The mixer can be moved to the host's audio thread.
    /// A sound task can only be connected once, even if the mixer has been dropped.
    pub fn connect_task_audio_output(&mut self, task: u32) -> Result<Mixer> {
        self.sound_output_state(task)?.connect_output()
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
